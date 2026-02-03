//! Agent scheduler for periodic and event-triggered execution.
//! Uses standard cron library for accurate cron expression parsing.

use crate::ai_agent::executor::AgentExecutor;
use chrono::{DateTime, Utc};
use chrono_tz::Tz;
use cron::Schedule;
use edge_ai_storage::{AiAgent, AgentSchedule, ScheduleType};
use std::collections::HashMap;
use std::sync::Arc;
use thiserror::Error;
use tokio::sync::RwLock;
use tokio::time::{interval, Duration};

/// Scheduler configuration.
#[derive(Debug, Clone)]
pub struct SchedulerConfig {
    /// Tick interval for checking scheduled tasks (milliseconds)
    pub tick_interval_ms: u64,
    /// Maximum concurrent executions
    pub max_concurrent: usize,
    /// Default timezone for cron expressions (IANA format, e.g., "Asia/Shanghai")
    pub default_timezone: Option<String>,
}

impl Default for SchedulerConfig {
    fn default() -> Self {
        Self {
            tick_interval_ms: 1000, // Check every second
            max_concurrent: 10,
            default_timezone: Some("Asia/Shanghai".to_string()),
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
    /// Default timezone parsed
    default_tz: Arc<RwLock<Option<Tz>>>,
    /// Semaphore for limiting concurrent executions
    execution_semaphore: Arc<tokio::sync::Semaphore>,
}

impl AgentScheduler {
    /// Create a new scheduler.
    pub async fn new(config: SchedulerConfig) -> Result<Self, crate::error::NeoTalkError> {
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

        Ok(Self {
            tasks: Arc::new(RwLock::new(HashMap::new())),
            config: Arc::new(RwLock::new(config)),
            running: Arc::new(RwLock::new(false)),
            running_executions: Arc::new(RwLock::new(std::collections::HashSet::new())),
            default_tz: Arc::new(RwLock::new(default_tz)),
            execution_semaphore: Arc::new(tokio::sync::Semaphore::new(max_concurrent)),
        })
    }

    /// Set the global default timezone.
    pub async fn set_default_timezone(&self, timezone: String) -> Result<(), SchedulerError> {
        let tz = timezone.parse::<Tz>().map_err(|_| SchedulerError::InvalidTimezone(timezone.clone()))?;

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
    pub async fn schedule_agent(&self, agent: AiAgent) -> Result<(), crate::error::NeoTalkError> {
        let (next_execution, cron_schedule) = self
            .calculate_next_execution(&agent.schedule)
            .await
            .map_err(|e| crate::NeoTalkError::Config(format!("Schedule error: {}", e)))?;

        let task = ScheduledTask {
            agent_id: agent.id.clone(),
            next_execution,
            interval_seconds: agent.schedule.interval_seconds,
            cron_expression: agent.schedule.cron_expression.clone(),
            cron_schedule,
            timezone: agent.schedule.timezone.clone(),
            enabled: agent.status == edge_ai_storage::AgentStatus::Active,
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
    pub async fn unschedule_agent(&self, agent_id: &str) -> Result<(), crate::error::NeoTalkError> {
        let mut tasks = self.tasks.write().await;
        tasks.remove(agent_id);
        drop(tasks);

        tracing::debug!(agent_id = %agent_id, "Agent unscheduled");
        Ok(())
    }

    /// Start the scheduler.
    pub async fn start(&self, executor: Arc<AgentExecutor>) -> Result<(), crate::error::NeoTalkError> {
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
        let executor_ref = executor;
        let semaphore = self.execution_semaphore.clone();

        tokio::spawn(async move {
            let mut ticker = interval(tick_interval);
            ticker.tick().await; // First tick completes immediately

            loop {
                ticker.tick().await;

                // Check if still running
                {
                    let flag = running_flag.read().await;
                    if !*flag {
                        break;
                    }
                }

                // Check for tasks to execute
                let now = Utc::now().timestamp();
                let tasks_to_execute = {
                    let mut tasks_guard = tasks.write().await;
                    let mut to_execute = Vec::new();

                    for (agent_id, task) in tasks_guard.iter_mut() {
                        if !task.enabled {
                            continue;
                        }

                        if now >= task.next_execution {
                            // Check concurrency limit
                            let running_count = running_executions.read().await.len();
                            if running_count >= max_concurrent {
                                tracing::warn!(
                                    "Scheduler at concurrency limit ({}/{}), skipping execution",
                                    running_count, max_concurrent
                                );
                                break;
                            }

                            // Mark as running
                            to_execute.push((agent_id.clone(), task.cron_schedule.clone()));

                            // Schedule next execution
                            Self::update_next_execution(task, now);
                        }
                    }

                    to_execute
                };

                // Execute tasks
                for (agent_id, _cron_schedule) in tasks_to_execute {
                    let executor = executor_ref.clone();
                    let running_executions_clone = running_executions.clone();
                    let semaphore_clone = semaphore.clone();

                    // Mark as running
                    running_executions_clone.write().await.insert(agent_id.clone());

                    tokio::spawn(async move {
                        // Acquire semaphore permit for concurrency control
                        // If semaphore is closed, log and skip execution
                        let _permit = match semaphore_clone.acquire().await {
                            Ok(p) => p,
                            Err(_) => {
                                tracing::error!(agent_id = %agent_id, "Semaphore closed, skipping agent execution");
                                running_executions_clone.write().await.remove(&agent_id);
                                return;
                            }
                        };

                        tracing::debug!(agent_id = %agent_id, "Executing scheduled agent");

                        match executor.store().get_agent(&agent_id).await {
                            Ok(Some(agent)) => {
                                let result = executor.execute_agent(agent).await;

                                match result {
                                    Ok(record) => {
                                        tracing::info!(
                                            agent_id = %agent_id,
                                            execution_id = %record.id,
                                            status = ?record.status,
                                            duration_ms = record.duration_ms,
                                            "Scheduled agent execution completed"
                                        );
                                    }
                                    Err(e) => {
                                        tracing::error!(
                                            agent_id = %agent_id,
                                            error = %e,
                                            "Scheduled agent execution failed"
                                        );
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

                        // Mark as no longer running
                        running_executions_clone.write().await.remove(&agent_id);
                    });
                }
            }

            tracing::info!("Agent scheduler stopped");
        });

        let config = self.config.read().await;
        tracing::info!("Agent scheduler started with default timezone: {:?}", config.default_timezone);
        Ok(())
    }

    /// Stop the scheduler.
    pub async fn stop(&self) -> Result<(), crate::error::NeoTalkError> {
        let mut running = self.running.write().await;
        *running = false;
        drop(running);

        // Wait for running executions to complete
        for _ in 0..30 {
            let count = self.running_executions.read().await.len();
            if count == 0 {
                break;
            }
            tracing::info!("Waiting for {} running executions to complete", count);
            tokio::time::sleep(Duration::from_millis(500)).await;
        }

        tracing::info!("Agent scheduler stopped");
        Ok(())
    }

    /// Get all scheduled tasks.
    pub async fn get_tasks(&self) -> Vec<ScheduledTask> {
        self.tasks.read().await.values().cloned().collect()
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
            ScheduleType::Interval => {
                let interval = schedule.interval_seconds.unwrap_or(300);
                Ok((now.timestamp() + interval as i64, None))
            }
            ScheduleType::Cron => {
                let cron_expr = schedule
                    .cron_expression
                    .as_ref()
                    .ok_or_else(|| SchedulerError::InvalidCronExpression("No cron expression provided".to_string()))?;

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
                let base_time_for_calc: chrono::DateTime<chrono::FixedOffset> = if let Some(ref tz) = tz_opt {
                    now.with_timezone(tz).fixed_offset()
                } else {
                    now.fixed_offset()
                };

                let next_execution = parsed
                    .after(&base_time_for_calc)
                    .next()
                    .ok_or_else(|| SchedulerError::CalculationError("Could not calculate next execution time".to_string()))?;

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
            ScheduleType::Once => {
                // Execute immediately
                Ok((now.timestamp(), None))
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
    fn update_next_execution(task: &mut ScheduledTask, now: i64) {
        if let Some(interval) = task.interval_seconds {
            // Simple interval: just add interval
            task.next_execution = now + interval as i64;
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
            // One-time execution, disable
            task.enabled = false;
        }
    }
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
    async fn test_once_schedule_immediate() {
        let scheduler = AgentScheduler::new(SchedulerConfig::default())
            .await
            .unwrap();

        let once_schedule = AgentSchedule {
            schedule_type: ScheduleType::Once,
            cron_expression: None,
            interval_seconds: None,
            event_filter: None,
            timezone: None,
        };

        let now = Utc::now().timestamp();
        let (next, _) = scheduler.calculate_next_execution(&once_schedule).await.unwrap();

        // Once schedules should execute immediately
        assert!(next >= now && next <= now + 5);
    }
}
