//! Agent scheduler for periodic and event-triggered execution.
//! Uses standard cron library for accurate cron expression parsing.

use crate::ai_agent::executor::AgentExecutor;
use chrono::{DateTime, Utc};
use chrono_tz::Tz;
use cron::Schedule;
use neomind_storage::{AgentSchedule, AiAgent, ScheduleType};
use std::collections::HashMap;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::Arc;
use thiserror::Error;
use tokio::sync::RwLock;
use tokio::time::{interval, Duration};

/// Shared per-LLM-backend concurrency limiter.
///
/// Maps a backend ID (e.g. "ollama-local") to a semaphore that limits how many
/// agents can use that backend concurrently. Both the scheduler (interval/cron)
/// and the executor (event-triggered) share the same instance so that all
/// execution paths are throttled uniformly.
#[derive(Clone)]
pub struct BackendSemaphores {
    inner: Arc<RwLock<HashMap<String, Arc<tokio::sync::Semaphore>>>>,
    permits_per_backend: usize,
}

impl BackendSemaphores {
    pub fn new(permits_per_backend: usize) -> Self {
        Self {
            inner: Arc::new(RwLock::new(HashMap::new())),
            permits_per_backend,
        }
    }

    /// Get or create the semaphore for a given backend ID.
    pub async fn get(&self, backend_id: &str) -> Arc<tokio::sync::Semaphore> {
        // Fast path: read lock
        {
            let guard = self.inner.read().await;
            if let Some(sem) = guard.get(backend_id) {
                return Arc::clone(sem);
            }
        }
        // Slow path: write lock
        let mut guard = self.inner.write().await;
        guard
            .entry(backend_id.to_string())
            .or_insert_with(|| Arc::new(tokio::sync::Semaphore::new(self.permits_per_backend)))
            .clone()
    }
}

/// Map of in-flight execution task handles, keyed by agent_id. Used by
/// `AgentScheduler::stop` to abort in-flight executions cooperatively.
type TaskHandleMap = Arc<RwLock<HashMap<String, tokio::task::JoinHandle<()>>>>;

/// RAII guard that releases a reserved slot in `running_executions` on drop.
///
/// This MUST cover *every* exit path from the scheduler's spawned task body:
/// normal completion, early `return`, task cancellation (future drop), and
/// panics (the default `tokio::spawn` catches unwinds and drops the future,
/// which runs this Drop). Without it, a panic or cancel between the initial
/// `insert` and the trailing manual `remove` would leak the slot; after
/// `max_concurrent` (10) leaks the scheduler silently skips every future
/// execution until process restart.
///
/// Also removes the entry from `running_task_handles` so that `stop()`'s
/// abort pass doesn't try to abort already-completed tasks.
///
/// `Drop` cannot `.await`, so we spawn the async removal onto the current
/// tokio runtime. If no runtime is available (process tearing down), the
/// removal is a no-op — acceptable because the scheduler is dying anyway
/// and the in-memory set is discarded with it.
struct RunningSlotGuard {
    agent_id: String,
    running: Arc<RwLock<std::collections::HashSet<String>>>,
    /// Optional join-handle map for cooperative cancellation via `stop()`.
    /// `None` for tests that don't track handles.
    task_handles: Option<TaskHandleMap>,
}

impl Drop for RunningSlotGuard {
    fn drop(&mut self) {
        let agent_id = std::mem::take(&mut self.agent_id);
        let running_arc = self.running.clone();
        let handles_arc = self.task_handles.clone();

        // B2 fix: prefer synchronous `try_write` removal over spawning an
        // async cleanup task. `try_write` succeeds in the uncontended common
        // case (no other task holds the lock), making cleanup deterministic
        // — no dependency on the runtime still being alive to poll the
        // spawned cleanup task. This matters during runtime shutdown where
        // a spawned task might be dropped before it runs.
        let mut running_done = false;
        let mut handles_done = handles_arc.is_none(); // None → nothing to clean

        if let Ok(mut guard) = running_arc.try_write() {
            guard.remove(&agent_id);
            running_done = true;
        }
        if let Some(map) = &handles_arc {
            if let Ok(mut guard) = map.try_write() {
                guard.remove(&agent_id);
                handles_done = true;
            }
        }

        if running_done && handles_done {
            return;
        }

        // Slow path: lock was contended — at that point someone is actively
        // holding the lock, which means the runtime is alive. Spawn an async
        // cleanup that will run once the lock is released.
        match tokio::runtime::Handle::try_current() {
            Ok(handle) => {
                handle.spawn(async move {
                    if !running_done {
                        running_arc.write().await.remove(&agent_id);
                    }
                    if !handles_done {
                        if let Some(map) = handles_arc {
                            map.write().await.remove(&agent_id);
                        }
                    }
                });
            }
            Err(_) => {
                // No runtime available — we can't await the lock. Log so
                // the leak is observable. This only fires during process
                // teardown (or Tauri desktop runtime reload), where the
                // in-memory state is discarded anyway. Slot leak is benign:
                // maps grow but `stop()` drains them on next call.
                tracing::warn!(
                    agent_id = %agent_id,
                    running_done,
                    handles_done,
                    "RunningSlotGuard::drop had no runtime to complete cleanup; \
                     in-memory slot may leak until process exit or next stop() call"
                );
            }
        }
    }
}

/// Scheduler configuration.
#[derive(Debug, Clone)]
pub struct SchedulerConfig {
    /// Tick interval for checking scheduled tasks (milliseconds)
    pub tick_interval_ms: u64,
    /// Maximum concurrent executions (global limit)
    pub max_concurrent: usize,
    /// Maximum concurrent executions per LLM backend
    pub max_concurrent_per_backend: usize,
    /// Default timezone for cron expressions (IANA format, e.g., "Asia/Shanghai")
    pub default_timezone: Option<String>,
}

impl Default for SchedulerConfig {
    fn default() -> Self {
        Self {
            tick_interval_ms: 1000, // Check every second
            max_concurrent: 10,
            max_concurrent_per_backend: 2,
            default_timezone: Some("Asia/Shanghai".to_string()),
        }
    }
}

impl SchedulerConfig {
    /// Apply environment overrides for concurrency limits.
    ///
    /// Tuning knob for resource-constrained edge devices (e.g. RK3576 with
    /// 4-8 GB RAM): the defaults (global 10 / per-backend 2) can OOM small
    /// devices when many agents fire at once.
    ///
    /// - `NEOMIND_MAX_CONCURRENT` (global execution limit, default 10)
    /// - `NEOMIND_MAX_CONCURRENT_PER_BACKEND` (per-LLM-backend, default 2)
    ///
    /// Unset or invalid values keep the defaults.
    pub fn with_env_concurrency(mut self) -> Self {
        if let Some(n) = parse_concurrency_env("NEOMIND_MAX_CONCURRENT") {
            self.max_concurrent = n;
        }
        if let Some(n) = parse_concurrency_env("NEOMIND_MAX_CONCURRENT_PER_BACKEND") {
            self.max_concurrent_per_backend = n;
        }
        self
    }
}

/// Parse a usize concurrency env override (must be >= 1 — 0 would deadlock the
/// semaphore forever). Invalid values are warned and ignored.
fn parse_concurrency_env(name: &str) -> Option<usize> {
    let raw = std::env::var(name).ok()?;
    match raw.trim().parse::<usize>() {
        Ok(n) if n >= 1 => Some(n),
        Ok(_) => {
            tracing::warn!(%name, value = %raw, "concurrency override must be >= 1, ignoring");
            None
        }
        Err(_) => {
            tracing::warn!(%name, value = %raw, "concurrency override is not a valid integer, ignoring");
            None
        }
    }
}

/// Error types for scheduler operations.
#[derive(Debug, Error)]
pub enum SchedulerError {
    #[error("Invalid cron expression: {0}")]
    InvalidCronExpression(String),

    #[error("Invalid timezone: {0}")]
    InvalidTimezone(String),

    #[error("Cron parsing error: {0}")]
    CronParseError(String),

    #[error("Schedule calculation error: {0}")]
    CalculationError(String),
}

/// A scheduled task.
#[derive(Debug, Clone)]
pub struct ScheduledTask {
    /// Agent ID
    pub agent_id: String,
    /// Next execution time (unix timestamp)
    pub next_execution: i64,
    /// Interval for recurring tasks (seconds)
    pub interval_seconds: Option<u64>,
    /// Cron expression (if applicable)
    pub cron_expression: Option<String>,
    /// Parsed cron schedule (cached)
    cron_schedule: Option<Schedule>,
    /// Timezone for this task
    pub timezone: Option<String>,
    /// Whether this task is enabled
    pub enabled: bool,
    /// Agent priority (0-255, higher = more urgent). Used to decide which due
    /// agents get execution slots when the concurrency limit is reached.
    pub priority: u8,
}

/// Agent scheduler for executing agents on schedule.
pub struct AgentScheduler {
    /// Scheduled tasks
    tasks: Arc<RwLock<HashMap<String, ScheduledTask>>>,
    /// Configuration
    config: Arc<RwLock<SchedulerConfig>>,
    /// Running state
    running: Arc<RwLock<bool>>,
    /// Currently running executions
    running_executions: Arc<RwLock<std::collections::HashSet<String>>>,
    /// JoinHandles for spawned execution tasks, keyed by agent_id.
    /// Used by `stop()` to abort in-flight executions cooperatively.
    /// Removed automatically by `RunningSlotGuard` on task completion.
    running_task_handles: TaskHandleMap,
    /// Default timezone parsed
    default_tz: Arc<RwLock<Option<Tz>>>,
    /// Semaphore for limiting concurrent executions
    execution_semaphore: Arc<tokio::sync::Semaphore>,
    /// Per-LLM-backend semaphores (shared with executor for event-triggered path)
    backend_semaphores: BackendSemaphores,
    /// Timestamp of the last successful tick (for health monitoring)
    last_tick: Arc<AtomicI64>,
}

impl AgentScheduler {
    /// Create a new scheduler.
    pub async fn new(config: SchedulerConfig) -> Result<Self, crate::error::NeoMindError> {
        // Parse default timezone
        let default_tz = if let Some(tz_str) = &config.default_timezone {
            match tz_str.parse::<Tz>() {
                Ok(tz) => Some(tz),
                Err(_) => {
                    tracing::warn!(
                        timezone = %tz_str,
                        "Invalid default timezone, falling back to UTC"
                    );
                    None
                }
            }
        } else {
            None
        };

        // Max concurrent executions from config (default 10)
        let max_concurrent = config.max_concurrent;
        let max_concurrent_per_backend = config.max_concurrent_per_backend;

        Ok(Self {
            tasks: Arc::new(RwLock::new(HashMap::new())),
            config: Arc::new(RwLock::new(config)),
            running: Arc::new(RwLock::new(false)),
            running_executions: Arc::new(RwLock::new(std::collections::HashSet::new())),
            running_task_handles: Arc::new(RwLock::new(HashMap::new())),
            default_tz: Arc::new(RwLock::new(default_tz)),
            execution_semaphore: Arc::new(tokio::sync::Semaphore::new(max_concurrent)),
            backend_semaphores: BackendSemaphores::new(max_concurrent_per_backend),
            last_tick: Arc::new(AtomicI64::new(0)),
        })
    }

    /// Get a reference to the shared backend semaphores (for wiring into executor).
    /// The global execution-count semaphore (default 10). Shared with the
    /// event-trigger path so event-spawned executions are accounted against
    /// the same global bound as scheduled ones (previously event executions
    /// only held a per-backend permit, so a burst could stack past the
    /// global limit).
    pub fn execution_semaphore(&self) -> &Arc<tokio::sync::Semaphore> {
        &self.execution_semaphore
    }

    pub fn backend_semaphores(&self) -> &BackendSemaphores {
        &self.backend_semaphores
    }

    /// Set the global default timezone.
    pub async fn set_default_timezone(&self, timezone: String) -> Result<(), SchedulerError> {
        let tz = timezone
            .parse::<Tz>()
            .map_err(|_| SchedulerError::InvalidTimezone(timezone.clone()))?;

        // Update the parsed timezone
        *self.default_tz.write().await = Some(tz);

        // Update the config
        let mut config = self.config.write().await;
        config.default_timezone = Some(timezone);

        tracing::info!("Default timezone updated to: {:?}", config.default_timezone);
        Ok(())
    }

    /// Get the current default timezone string.
    pub async fn get_default_timezone(&self) -> Option<String> {
        self.config.read().await.default_timezone.clone()
    }

    /// Schedule an agent for execution.
    pub async fn schedule_agent(&self, agent: AiAgent) -> Result<(), crate::error::NeoMindError> {
        let (next_execution, cron_schedule) = self
            .calculate_next_execution(&agent.schedule)
            .await
            .map_err(|e| crate::NeoMindError::Config(format!("Schedule error: {}", e)))?;

        let task = ScheduledTask {
            agent_id: agent.id.clone(),
            next_execution,
            interval_seconds: agent.schedule.interval_seconds,
            cron_expression: agent.schedule.cron_expression.clone(),
            cron_schedule,
            timezone: agent.schedule.timezone.clone(),
            enabled: agent.status == neomind_storage::AgentStatus::Active,
            priority: agent.priority,
        };

        let agent_id = agent.id.clone();
        let mut tasks = self.tasks.write().await;
        tasks.insert(agent_id.clone(), task);
        drop(tasks);

        tracing::debug!(
            agent_id = %agent_id,
            next_execution = next_execution,
            "Agent scheduled"
        );

        Ok(())
    }

    /// Unschedule an agent.
    pub async fn unschedule_agent(&self, agent_id: &str) -> Result<(), crate::error::NeoMindError> {
        let mut tasks = self.tasks.write().await;
        tasks.remove(agent_id);
        drop(tasks);

        tracing::debug!(agent_id = %agent_id, "Agent unscheduled");
        Ok(())
    }

    /// Start the scheduler.
    pub async fn start(
        &self,
        executor: Arc<AgentExecutor>,
    ) -> Result<(), crate::error::NeoMindError> {
        let mut running = self.running.write().await;
        if *running {
            return Ok(());
        }
        *running = true;
        drop(running);

        // Read config values before spawning the task
        let config = self.config.read().await;
        let tick_interval = Duration::from_millis(config.tick_interval_ms);
        let max_concurrent = config.max_concurrent;
        drop(config);

        let tasks = self.tasks.clone();
        let running_flag = self.running.clone();
        let running_executions = self.running_executions.clone();
        let running_task_handles = self.running_task_handles.clone();
        let executor_ref = executor;
        let semaphore = self.execution_semaphore.clone();
        let backend_semaphores = self.backend_semaphores.clone();
        let last_tick = self.last_tick.clone();

        tokio::spawn(async move {
            let mut ticker = interval(tick_interval);
            ticker.tick().await; // First tick completes immediately

            loop {
                ticker.tick().await;

                // Update heartbeat timestamp for health monitoring
                last_tick.store(Utc::now().timestamp(), Ordering::Relaxed);

                // Check if still running
                {
                    let flag = running_flag.read().await;
                    if !*flag {
                        break;
                    }
                }

                // Check for tasks to execute
                let now = Utc::now().timestamp();
                // Reservation phase runs in its OWN task: a panic inside
                // (e.g. an arithmetic bug on a bad schedule value) is
                // confined to the child and logged, instead of unwinding
                // this loop — one panicking task used to silently kill the
                // scheduler and stop EVERY agent on the platform.
                let reserve = tokio::spawn(reserve_due_tasks(
                    tasks.clone(),
                    running_executions.clone(),
                    now,
                    max_concurrent,
                ));
                let (tasks_to_execute, skipped_tasks) = match reserve.await {
                    Ok(result) => result,
                    Err(join_err) => {
                        if join_err.is_panic() {
                            tracing::error!(
                                error = %join_err,
                                "Scheduler tick reservation panicked — tick skipped, scheduler loop alive"
                            );
                        }
                        continue;
                    }
                };

                // Log summary of skipped tasks (if any)
                if !skipped_tasks.is_empty() {
                    tracing::warn!(
                        skipped_count = skipped_tasks.len(),
                        "Skipped agent executions due to concurrency limit"
                    );
                }

                // Execute tasks
                for (agent_id, _cron_schedule) in tasks_to_execute {
                    let executor = executor_ref.clone();
                    let running_executions_clone = running_executions.clone();
                    let task_handles_clone = running_task_handles.clone();
                    let semaphore_clone = semaphore.clone();
                    let backend_sems = backend_semaphores.clone();

                    // Note: agent_id was already inserted into running_executions
                    // during the reservation phase (line 411) under the scheduler lock.
                    // RunningSlotGuard below handles cleanup on task exit.

                    // Capture agent_id for the handle map BEFORE the value is
                    // moved into the spawn closure below.
                    let agent_id_for_handle_map = agent_id.clone();

                    // B1 fix: hold `running_task_handles.write()` across
                    // spawn()+insert so `stop()`'s drain can't observe an
                    // empty map and skip this task. The lock is held only
                    // briefly — `tokio::spawn` is synchronous (queues the
                    // task, doesn't poll it), and the spawned task cannot
                    // start until we release the lock (it needs no locks
                    // to start, but it can't pre-empt us on the same
                    // runtime thread). `stop()` blocking on this write
                    // lock is exactly the synchronization we want.
                    {
                        let mut handles_guard = running_task_handles.write().await;
                        handles_guard.insert(
                            agent_id_for_handle_map.clone(),
                            tokio::spawn(async move {
                        // RAII guard: removes the agent_id from running_executions
                        // AND from running_task_handles on *every* exit path
                        // (normal return, early return, cancellation, or panic).
                        // Placed at the very top so its Drop runs last. See
                        // RunningSlotGuard docs for why this is required instead
                        // of a trailing manual remove().
                        let _slot_guard = RunningSlotGuard {
                            agent_id: agent_id.clone(),
                            running: running_executions_clone.clone(),
                            task_handles: Some(task_handles_clone.clone()),
                        };

                        // Acquire semaphore permit for concurrency control
                        // If semaphore is closed, log and skip execution
                        let _permit = match semaphore_clone.acquire().await {
                            Ok(p) => p,
                            Err(_) => {
                                tracing::error!(agent_id = %agent_id, "Semaphore closed, skipping agent execution");
                                return;
                            }
                        };

                        tracing::debug!(agent_id = %agent_id, "Executing scheduled agent");

                        match executor.store().get_agent(&agent_id).await {
                            Ok(Some(agent)) => {
                                // Acquire per-backend semaphore (WAIT, not fail)
                                let backend_id = agent
                                    .llm_backend_id
                                    .clone()
                                    .unwrap_or_else(|| "default".to_string());
                                let backend_sem = backend_sems.get(&backend_id).await;
                                let available = backend_sem.available_permits();
                                if available == 0 {
                                    tracing::debug!(
                                        agent_id = %agent_id,
                                        backend_id = %backend_id,
                                        "Waiting for backend permit"
                                    );
                                }
                                let _backend_permit = match backend_sem.acquire().await {
                                    Ok(p) => p,
                                    Err(_) => {
                                        tracing::warn!(agent_id = %agent_id, "Backend semaphore closed");
                                        return;
                                    }
                                };
                                tracing::debug!(
                                    agent_id = %agent_id,
                                    backend_id = %backend_id,
                                    "Acquired backend permit"
                                );

                                let max_retries = agent.max_retries;
                                let consecutive = agent.consecutive_failures;

                                let result =
                                    executor.execute_agent(agent.clone(), None, None).await;

                                match result {
                                    Ok(record) => {
                                        tracing::info!(
                                            agent_id = %agent_id,
                                            execution_id = %record.id,
                                            status = ?record.status,
                                            duration_ms = record.duration_ms,
                                            "Scheduled agent execution completed"
                                        );

                                        // Reset consecutive failures on success
                                        if consecutive > 0 {
                                            if let Err(e) = executor
                                                .store()
                                                .update_agent_consecutive_failures(&agent_id, 0)
                                                .await
                                            {
                                                tracing::warn!(
                                                    agent_id = %agent_id,
                                                    error = %e,
                                                    "Failed to reset consecutive failure count"
                                                );
                                            }
                                        }
                                    }
                                    Err(e) => {
                                        let new_consecutive = consecutive + 1;

                                        tracing::error!(
                                            agent_id = %agent_id,
                                            error = %e,
                                            consecutive_failures = new_consecutive,
                                            max_retries = max_retries,
                                            "Scheduled agent execution failed"
                                        );

                                        // Update consecutive failure count
                                        if let Err(err) = executor
                                            .store()
                                            .update_agent_consecutive_failures(
                                                &agent_id,
                                                new_consecutive,
                                            )
                                            .await
                                        {
                                            tracing::warn!(
                                                agent_id = %agent_id,
                                                error = %err,
                                                "Failed to update consecutive failure count"
                                            );
                                        }

                                        // Retry if within limit
                                        if new_consecutive <= max_retries && max_retries > 0 {
                                            tracing::info!(
                                                agent_id = %agent_id,
                                                attempt = new_consecutive,
                                                max_retries = max_retries,
                                                "Retrying agent execution after failure"
                                            );

                                            // Exponential backoff: 5s, 10s, 20s...
                                            let backoff = std::time::Duration::from_secs(
                                                5 * 2_u64.pow(new_consecutive.saturating_sub(1)),
                                            );
                                            tokio::time::sleep(backoff).await;

                                            match executor.execute_agent(agent, None, None).await {
                                                Ok(retry_record) => {
                                                    tracing::info!(
                                                        agent_id = %agent_id,
                                                        execution_id = %retry_record.id,
                                                        attempt = new_consecutive,
                                                        "Agent retry succeeded"
                                                    );
                                                    // Reset failures on retry success
                                                    let _ = executor
                                                        .store()
                                                        .update_agent_consecutive_failures(
                                                            &agent_id, 0,
                                                        )
                                                        .await;
                                                }
                                                Err(retry_err) => {
                                                    tracing::error!(
                                                        agent_id = %agent_id,
                                                        attempt = new_consecutive,
                                                        error = %retry_err,
                                                        "Agent retry failed"
                                                    );
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                            Ok(None) => {
                                tracing::warn!(agent_id = %agent_id, "Agent not found");
                            }
                            Err(e) => {
                                tracing::error!(
                                    agent_id = %agent_id,
                                    error = %e,
                                    "Failed to get agent for execution"
                                );
                            }
                        }

                        // `_slot_guard` drops here and removes the agent_id
                        // from running_executions AND running_task_handles on
                        // every exit path.
                        }),
                        );
                        // Lock released here. Any concurrent `stop()` call
                        // that blocked on the write lock will now drain the
                        // map and observe this freshly-inserted handle.
                    }
                }
            }

            tracing::info!("Agent scheduler stopped");
        });

        let config = self.config.read().await;
        tracing::info!(
            "Agent scheduler started with default timezone: {:?}",
            config.default_timezone
        );
        Ok(())
    }

    /// Stop the scheduler.
    pub async fn stop(&self) -> Result<(), crate::error::NeoMindError> {
        let mut running = self.running.write().await;
        *running = false;
        drop(running);

        // Cooperatively abort all in-flight executions. `JoinHandle::abort`
        // cancels the task at its next `.await` point — for our path this
        // means:
        //   - In-flight HTTP request (LLM call): reqwest future drops →
        //     connection closes, request aborted.
        //   - In-flight shell tool: SubprocessGuard drops → killpg kills the
        //     subprocess (whole process group, including grandchildren).
        //   - Other tools: future drops → cleanup via Drop impls.
        // The aborted task's future unwind triggers `RunningSlotGuard::drop`,
        // which removes the agent_id from `running_executions` and from
        // `running_task_handles`.
        let handles_to_abort: Vec<tokio::task::JoinHandle<()>> = {
            let mut map = self.running_task_handles.write().await;
            map.drain().map(|(_, h)| h).collect()
        };
        if !handles_to_abort.is_empty() {
            tracing::info!(
                count = handles_to_abort.len(),
                "Aborting in-flight agent executions"
            );
            for handle in &handles_to_abort {
                handle.abort();
            }
        }

        // Wait for running executions to observe the abort and unwind. The
        // 15s budget (30 × 500ms) matches the prior behavior as a hard upper
        // bound; in practice abort propagation takes <1s.
        for _ in 0..30 {
            let count = self.running_executions.read().await.len();
            if count == 0 {
                break;
            }
            tracing::info!(count, "Waiting for aborted agent executions to unwind");
            tokio::time::sleep(Duration::from_millis(500)).await;
        }

        tracing::info!("Agent scheduler stopped");
        Ok(())
    }

    /// Validate a cron expression and return next execution times.
    pub fn validate_cron(
        &self,
        expression: &str,
        timezone: Option<&str>,
    ) -> Result<Vec<DateTime<Utc>>, SchedulerError> {
        let schedule = Self::parse_cron_expression(expression)?;
        let tz_opt = Self::parse_timezone(timezone);

        let now = Utc::now();
        // Use FixedOffset for unified calculation
        let datetime: chrono::DateTime<chrono::FixedOffset> = if let Some(tz) = tz_opt {
            now.with_timezone(&tz).fixed_offset()
        } else {
            now.fixed_offset()
        };

        let mut next_executions = Vec::with_capacity(5);
        let mut upcoming = schedule.after(&datetime);

        for _ in 0..5 {
            if let Some(next) = upcoming.next() {
                // Convert back to UTC for consistency
                let utc_next = next.with_timezone(&Utc);
                next_executions.push(utc_next);
            } else {
                break;
            }
        }

        Ok(next_executions)
    }

    /// Calculate the next execution time for a schedule.
    /// Returns (timestamp, parsed_schedule) where parsed_schedule is Some() for cron schedules.
    pub(crate) async fn calculate_next_execution(
        &self,
        schedule: &AgentSchedule,
    ) -> Result<(i64, Option<Schedule>), SchedulerError> {
        let now = Utc::now();

        match schedule.schedule_type {
            // Manual-only: never auto-scheduled. The agent runs via manual
            // invoke/execute (repeatable) and shows Completed while idle.
            // A never-due time keeps the task registered but inert (enabled
            // flag also gates, this is belt-and-suspenders).
            ScheduleType::Manual => Ok((i64::MAX, None)),
            ScheduleType::Interval => {
                // `interval_seconds: Some(0)` is the frontend's "on-demand"
                // convention (AgentEditorFullScreen: manual-only agent, no
                // auto-trigger). Map it to a next_execution that never comes
                // due — the agent runs only via manual invoke/execute.
                // i64::MAX is safe: the tick loop compares
                // `now >= task.next_execution`, and arithmetic below only
                // ever reads this value.
                if schedule.interval_seconds == Some(0) {
                    return Ok((i64::MAX, None));
                }
                let interval = schedule.interval_seconds.unwrap_or(300);
                Ok((now.timestamp() + interval as i64, None))
            }
            ScheduleType::Cron => {
                let cron_expr = schedule.cron_expression.as_ref().ok_or_else(|| {
                    SchedulerError::InvalidCronExpression("No cron expression provided".to_string())
                })?;

                let parsed = Self::parse_cron_expression(cron_expr)?;

                // Get timezone for this schedule - read from RwLock
                let default_tz = self.default_tz.read().await;
                let tz_opt = schedule
                    .timezone
                    .as_ref()
                    .and_then(|tz_str| Self::parse_timezone(Some(tz_str)))
                    .or(*default_tz);
                drop(default_tz);

                // Calculate next execution time
                // We need to use a unified DateTime type for the calculation
                let base_time_for_calc: chrono::DateTime<chrono::FixedOffset> =
                    if let Some(ref tz) = tz_opt {
                        now.with_timezone(tz).fixed_offset()
                    } else {
                        now.fixed_offset()
                    };

                let next_execution = parsed.after(&base_time_for_calc).next().ok_or_else(|| {
                    SchedulerError::CalculationError(
                        "Could not calculate next execution time".to_string(),
                    )
                })?;

                // Convert to UTC timestamp
                let next_timestamp = next_execution.timestamp();

                // Ensure next execution is in the future
                let now_ts = now.timestamp();
                let final_timestamp = if next_timestamp <= now_ts {
                    now_ts + 60 // At least 1 minute in the future
                } else {
                    next_timestamp
                };

                Ok((final_timestamp, Some(parsed)))
            }
            ScheduleType::Event => {
                // Event-triggered, no scheduled execution
                Ok((i64::MAX, None))
            }
        }
    }

    /// Parse a cron expression using the standard cron library.
    fn parse_cron_expression(expression: &str) -> Result<Schedule, SchedulerError> {
        expression
            .parse::<Schedule>()
            .map_err(|e| SchedulerError::CronParseError(e.to_string()))
    }

    /// Parse a timezone string.
    fn parse_timezone(timezone: Option<&str>) -> Option<Tz> {
        let tz_str = timezone.unwrap_or("UTC");
        tz_str.parse::<Tz>().ok()
    }

    /// Update the next execution time for a task after execution.
    ///
    /// For interval tasks, this calculates the next execution based on the
    /// previously scheduled time (not the actual execution time) to prevent
    /// time drift. If the next scheduled time has already passed, it will
    /// calculate forward from the current time.
    fn update_next_execution(task: &mut ScheduledTask, now: i64) {
        if let Some(interval) = task.interval_seconds {
            // `Some(0)` = on-demand (manual-only). Reschedule to "never" —
            // the division below would otherwise divide by zero and PANIC,
            // killing the whole scheduler loop (one bad task used to stop
            // every agent on the platform). Legacy rows with 0 may still be
            // in storage, so guard defensively on every path.
            if interval == 0 {
                task.next_execution = i64::MAX;
                return;
            }
            // Interval: use the previous scheduled time to prevent drift
            // E.g., if scheduled for 8:00 but executed at 8:00:01,
            // next should be 8:05 (not 8:05:01)
            let scheduled_next = task.next_execution + interval as i64;

            if scheduled_next > now {
                // The next scheduled time is still in the future - use it
                task.next_execution = scheduled_next;
            } else {
                // We've fallen behind (e.g., system was paused/sleeping)
                // Calculate the next future time from now
                let intervals_behind = (now - scheduled_next) / interval as i64 + 1;
                task.next_execution = scheduled_next + (intervals_behind * interval as i64);

                tracing::debug!(
                    agent_id = %task.agent_id,
                    scheduled_next = scheduled_next,
                    now = now,
                    intervals_behind = intervals_behind,
                    new_next = task.next_execution,
                    "Interval task fell behind, rescheduled"
                );
            }
        } else if let Some(ref schedule) = task.cron_schedule {
            // Cron: calculate next occurrence
            let base_time = Utc::now();
            let tz_opt = task
                .timezone
                .as_ref()
                .and_then(|tz_str| tz_str.parse::<Tz>().ok());

            // Use FixedOffset for consistent calculation
            let calc_base: chrono::DateTime<chrono::FixedOffset> = if let Some(ref tz) = tz_opt {
                base_time.with_timezone(tz).fixed_offset()
            } else {
                base_time.fixed_offset()
            };

            if let Some(next) = schedule.after(&calc_base).next() {
                task.next_execution = next.timestamp();
            } else {
                // No more executions, disable
                task.enabled = false;
            }
        } else {
            // No interval and no cron schedule - invalid state, disable task
            // This should not happen with properly initialized tasks
            task.enabled = false;
        }
    }
}

/// One scheduler tick's reservation phase: collect due tasks, sort by
/// priority, apply the concurrency limit, reserve slots, and advance
/// `next_execution`. Runs inside its own spawned task (see the tick loop) so
/// a panic here cannot kill the scheduler loop.
///
/// Extracted verbatim from the tick loop; behavior is unchanged.
async fn reserve_due_tasks(
    tasks: Arc<RwLock<HashMap<String, ScheduledTask>>>,
    running_executions: Arc<RwLock<std::collections::HashSet<String>>>,
    now: i64,
    max_concurrent: usize,
) -> (Vec<(String, Option<Schedule>)>, Vec<(String, i64, i64)>) {
    let mut tasks_guard = tasks.write().await;
    let mut running_guard = running_executions.write().await;

    // 1. Collect due agent IDs
    let mut due: Vec<String> = Vec::new();
    for (agent_id, task) in tasks_guard.iter() {
        if !task.enabled {
            continue;
        }
        if now >= task.next_execution {
            due.push(agent_id.clone());
        }
    }

    // 2. Sort by priority descending — when the concurrency limit
    //    is reached, high-priority agents get the slots first
    //    (was: random HashMap iteration order → dead priority field).
    due.sort_by(|a, b| {
        let pa = tasks_guard.get(a).map(|t| t.priority).unwrap_or(0);
        let pb = tasks_guard.get(b).map(|t| t.priority).unwrap_or(0);
        pb.cmp(&pa)
    });

    // 3. Apply concurrency limit + reserve slots
    let mut to_execute = Vec::new();
    let mut skipped: Vec<(String, i64, i64)> = Vec::new();
    for agent_id in &due {
        if running_guard.len() >= max_concurrent {
            let next_exec = tasks_guard
                .get(agent_id)
                .map(|t| t.next_execution)
                .unwrap_or(now);
            skipped.push((agent_id.clone(), next_exec, now - next_exec));
            tracing::warn!(
                agent_id = %agent_id,
                running_count = running_guard.len(),
                max_concurrent = max_concurrent,
                overdue_seconds = now - next_exec,
                "Scheduler at concurrency limit, skipping agent execution"
            );
            continue;
        }
        running_guard.insert(agent_id.clone());
        let cron = if let Some(task) = tasks_guard.get_mut(agent_id) {
            let cron = task.cron_schedule.clone();
            AgentScheduler::update_next_execution(task, now);
            cron
        } else {
            None
        };
        to_execute.push((agent_id.clone(), cron));
    }

    (to_execute, skipped)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_calculate_next_execution_interval() {
        let scheduler = AgentScheduler::new(SchedulerConfig::default())
            .await
            .unwrap();

        let interval_schedule = AgentSchedule {
            schedule_type: ScheduleType::Interval,
            cron_expression: None,
            interval_seconds: Some(300),
            event_filter: None,
            timezone: None,
        };

        let now = Utc::now().timestamp();
        let (next, _) = scheduler
            .calculate_next_execution(&interval_schedule)
            .await
            .unwrap();

        assert!(next >= now + 299 && next <= now + 301);
    }

    /// `ScheduleType::Manual` = manual-only: never auto-due.
    #[tokio::test]
    async fn test_calculate_next_execution_manual_never_due() {
        let scheduler = AgentScheduler::new(SchedulerConfig::default())
            .await
            .unwrap();

        let once = AgentSchedule {
            schedule_type: ScheduleType::Manual,
            cron_expression: None,
            interval_seconds: None,
            event_filter: None,
            timezone: None,
        };

        let (next, _) = scheduler.calculate_next_execution(&once).await.unwrap();
        assert_eq!(
            next,
            i64::MAX,
            "Manual tasks must never come due on their own"
        );
    }

    /// `interval_seconds: Some(0)` is the on-demand (manual-only) convention.
    /// It must schedule to a never-due time — NOT `now + 0`, which made the
    /// agent due on the very next tick.
    #[tokio::test]
    async fn test_calculate_next_execution_interval_zero_is_manual_only() {
        let scheduler = AgentScheduler::new(SchedulerConfig::default())
            .await
            .unwrap();

        let on_demand = AgentSchedule {
            schedule_type: ScheduleType::Interval,
            cron_expression: None,
            interval_seconds: Some(0),
            event_filter: None,
            timezone: None,
        };

        let (next, _) = scheduler
            .calculate_next_execution(&on_demand)
            .await
            .unwrap();
        assert_eq!(
            next,
            i64::MAX,
            "interval=0 (on-demand) must never come due on its own"
        );
    }

    /// THE panic regression: an interval-0 task that comes due used to hit
    /// `(now - scheduled_next) / 0` in `update_next_execution` — a division
    /// by zero that killed the entire scheduler loop (every agent on the
    /// platform silently stopped). It must now reschedule to never-due.
    #[tokio::test]
    async fn test_update_next_execution_interval_zero_does_not_panic() {
        let mut task = ScheduledTask {
            agent_id: "agent-on-demand".to_string(),
            next_execution: Utc::now().timestamp() - 30, // already due
            interval_seconds: Some(0),
            cron_expression: None,
            cron_schedule: None,
            timezone: None,
            enabled: true,
            priority: 5,
        };

        AgentScheduler::update_next_execution(&mut task, Utc::now().timestamp());

        assert_eq!(
            task.next_execution,
            i64::MAX,
            "interval=0 task must be pushed to never-due, not divide by zero"
        );
    }

    #[tokio::test]
    async fn test_calculate_next_execution_cron_hourly() {
        let scheduler = AgentScheduler::new(SchedulerConfig::default())
            .await
            .unwrap();

        // 6-field cron format: sec min hour day month weekday
        let cron_schedule = AgentSchedule {
            schedule_type: ScheduleType::Cron,
            cron_expression: Some("0 0 * * * *".to_string()), // Every hour at :00:00
            interval_seconds: None,
            event_filter: None,
            timezone: None,
        };

        let now = Utc::now().timestamp();
        let (next, parsed) = scheduler
            .calculate_next_execution(&cron_schedule)
            .await
            .unwrap();

        // Should have parsed the cron expression
        assert!(parsed.is_some());

        // Next execution should be within the next hour
        assert!(next > now && next <= now + 3600);
    }

    #[tokio::test]
    async fn test_calculate_next_execution_cron_daily() {
        let scheduler = AgentScheduler::new(SchedulerConfig::default())
            .await
            .unwrap();

        // 6-field cron format: sec min hour day month weekday
        let cron_schedule = AgentSchedule {
            schedule_type: ScheduleType::Cron,
            cron_expression: Some("0 0 8 * * *".to_string()), // Daily at 8:00:00 AM
            interval_seconds: None,
            event_filter: None,
            timezone: Some("Asia/Shanghai".to_string()),
        };

        let now = Utc::now().timestamp();
        let (next, parsed) = scheduler
            .calculate_next_execution(&cron_schedule)
            .await
            .unwrap();

        // Should have parsed the cron expression
        assert!(parsed.is_some());

        // Next execution should be in the future
        assert!(next > now);
    }

    #[tokio::test]
    async fn test_validate_cron_expression() {
        let scheduler = AgentScheduler::new(SchedulerConfig::default())
            .await
            .unwrap();

        // Valid 6-field cron expression
        let result = scheduler.validate_cron("0 0 * * * *", None);
        assert!(result.is_ok());
        let next_times = result.unwrap();
        assert!(!next_times.is_empty());
        assert!(next_times.len() <= 5);

        // Invalid cron expression
        let result = scheduler.validate_cron("invalid cron", None);
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_validate_cron_with_timezone() {
        let scheduler = AgentScheduler::new(SchedulerConfig::default())
            .await
            .unwrap();

        // Test with Shanghai timezone (6-field format)
        let result = scheduler.validate_cron("0 0 8 * * *", Some("Asia/Shanghai"));
        assert!(result.is_ok());
        let next_times = result.unwrap();
        assert!(!next_times.is_empty());
    }

    #[tokio::test]
    async fn test_invalid_cron_expression() {
        let scheduler = AgentScheduler::new(SchedulerConfig::default())
            .await
            .unwrap();

        let cron_schedule = AgentSchedule {
            schedule_type: ScheduleType::Cron,
            cron_expression: Some("invalid-cron".to_string()),
            interval_seconds: None,
            event_filter: None,
            timezone: None,
        };

        let result = scheduler.calculate_next_execution(&cron_schedule).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_event_schedule_no_execution() {
        let scheduler = AgentScheduler::new(SchedulerConfig::default())
            .await
            .unwrap();

        let event_schedule = AgentSchedule {
            schedule_type: ScheduleType::Event,
            cron_expression: None,
            interval_seconds: None,
            event_filter: None,
            timezone: None,
        };

        let (next, _) = scheduler
            .calculate_next_execution(&event_schedule)
            .await
            .unwrap();

        // Event schedules should have MAX next execution
        assert_eq!(next, i64::MAX);
    }

    #[tokio::test]
    async fn test_update_next_execution_interval_no_drift() {
        // Test that interval tasks don't drift due to execution delays
        let interval: u64 = 300; // 5 minutes

        // Use a time in the near future (within a few hours from now)
        let now = Utc::now().timestamp();
        let scheduled_time = now + 3600; // 1 hour from now

        // Simulate: scheduled at scheduled_time, but executed 5 seconds late
        let execution_time: i64 = scheduled_time + 5;

        let mut task = ScheduledTask {
            agent_id: "test-agent".to_string(),
            next_execution: scheduled_time,
            interval_seconds: Some(interval),
            cron_expression: None,
            cron_schedule: None,
            timezone: None,
            enabled: true,
            priority: 128,
        };

        // Update next execution (this should use scheduled time, not execution time)
        AgentScheduler::update_next_execution(&mut task, execution_time);

        // Next execution should be exactly 5 minutes after the SCHEDULED time
        // not 5 minutes after the EXECUTION time
        // This prevents drift: if we used execution_time, next would be scheduled_time + 305
        // With our fix, next is scheduled_time + 300 (exactly on schedule)
        assert_eq!(task.next_execution, scheduled_time + interval as i64);

        // Verify it's NOT based on execution time (which would cause drift)
        assert_ne!(task.next_execution, execution_time + interval as i64);
    }

    #[tokio::test]
    async fn test_update_next_execution_interval_recovery_after_delay() {
        // Test recovery when system is down/sleeping and misses executions
        let interval: u64 = 300; // 5 minutes

        // Use a time in the near future
        let now = Utc::now().timestamp();
        let scheduled_time = now + 3600; // 1 hour from now (8:00 AM equivalent)

        let mut task = ScheduledTask {
            agent_id: "test-agent".to_string(),
            next_execution: scheduled_time,
            interval_seconds: Some(interval),
            cron_expression: None,
            cron_schedule: None,
            timezone: None,
            enabled: true,
            priority: 128,
        };

        // Simulate first update (scheduled_time -> scheduled_time + 5 min)
        AgentScheduler::update_next_execution(&mut task, scheduled_time);
        let first_next = task.next_execution;
        assert_eq!(first_next, scheduled_time + interval as i64);

        // Now simulate a late execution - the task was supposed to run at first_next
        // but we're executing it much later
        // Sleep a tiny bit to ensure current time progresses
        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

        task.next_execution = first_next; // Keep first_next which is now in the past
        let current_time = Utc::now().timestamp();

        AgentScheduler::update_next_execution(&mut task, current_time);

        // Since first_next is in the past, scheduler should have calculated forward
        // to a future time based on current time
        assert!(task.next_execution > first_next);
        // Should be approximately in the future (allow for test timing variance)
        assert!(task.next_execution > current_time - 100);
    }

    #[tokio::test]
    async fn test_interval_schedule_stability() {
        // Test that multiple interval updates maintain stability
        let interval: u64 = 60; // 1 minute

        // Start from a time in the future
        let now = Utc::now().timestamp();
        let base_time = now + 3600; // 1 hour from now

        let mut task = ScheduledTask {
            agent_id: "test-agent".to_string(),
            next_execution: base_time,
            interval_seconds: Some(interval),
            cron_expression: None,
            cron_schedule: None,
            timezone: None,
            enabled: true,
            priority: 128,
        };

        // Simulate 10 executions, each with varying delays
        let delays: Vec<i64> = vec![0, 1, 2, 5, 10, 0, 3, 1, 0, 2];

        for (i, delay) in delays.iter().enumerate() {
            let execution_time = task.next_execution + delay;

            // Store the expected next execution before updating
            let expected = base_time + (i as i64 + 1) * interval as i64;

            AgentScheduler::update_next_execution(&mut task, execution_time);

            // The next execution should ALWAYS be on the planned schedule
            // regardless of execution delays
            assert_eq!(
                task.next_execution,
                expected,
                "After execution {}, next execution should be exactly on schedule",
                i + 1
            );
        }
    }
}
