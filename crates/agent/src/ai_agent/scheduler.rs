//! Agent scheduler for periodic and event-triggered execution.

use crate::ai_agent::executor::AgentExecutor;
use edge_ai_storage::{AiAgent, AgentSchedule, ScheduleType};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio::time::{interval, Duration};

/// Scheduler configuration.
#[derive(Debug, Clone)]
pub struct SchedulerConfig {
    /// Tick interval for checking scheduled tasks (milliseconds)
    pub tick_interval_ms: u64,
    /// Maximum concurrent executions
    pub max_concurrent: usize,
}

impl Default for SchedulerConfig {
    fn default() -> Self {
        Self {
            tick_interval_ms: 1000, // Check every second
            max_concurrent: 10,
        }
    }
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
    /// Whether this task is enabled
    pub enabled: bool,
}

/// Agent scheduler for executing agents on schedule.
pub struct AgentScheduler {
    /// Scheduled tasks
    tasks: Arc<RwLock<HashMap<String, ScheduledTask>>>,
    /// Configuration
    config: SchedulerConfig,
    /// Running state
    running: Arc<RwLock<bool>>,
    /// Currently running executions
    running_executions: Arc<RwLock<std::collections::HashSet<String>>>,
}

impl AgentScheduler {
    /// Create a new scheduler.
    pub async fn new(config: SchedulerConfig) -> Result<Self, crate::AgentError> {
        Ok(Self {
            tasks: Arc::new(RwLock::new(HashMap::new())),
            config,
            running: Arc::new(RwLock::new(false)),
            running_executions: Arc::new(RwLock::new(std::collections::HashSet::new())),
        })
    }

    /// Schedule an agent for execution.
    pub async fn schedule_agent(&self, agent: AiAgent) -> Result<(), crate::AgentError> {
        let next_execution = self.calculate_next_execution(&agent.schedule);

        let task = ScheduledTask {
            agent_id: agent.id.clone(),
            next_execution,
            interval_seconds: agent.schedule.interval_seconds,
            cron_expression: agent.schedule.cron_expression.clone(),
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
    pub async fn unschedule_agent(&self, agent_id: &str) -> Result<(), crate::AgentError> {
        let mut tasks = self.tasks.write().await;
        tasks.remove(agent_id);
        drop(tasks);

        tracing::debug!(agent_id = %agent_id, "Agent unscheduled");
        Ok(())
    }

    /// Start the scheduler.
    pub async fn start(&self, executor: Arc<AgentExecutor>) -> Result<(), crate::AgentError> {
        let mut running = self.running.write().await;
        if *running {
            return Ok(());
        }
        *running = true;
        drop(running);

        let tasks = self.tasks.clone();
        let running_flag = self.running.clone();
        let running_executions = self.running_executions.clone();
        let tick_interval = Duration::from_millis(self.config.tick_interval_ms);
        let max_concurrent = self.config.max_concurrent;
        let executor_ref = executor;

        tokio::spawn(async move {
            let mut ticker = interval(tick_interval);

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
                let now = chrono::Utc::now().timestamp();
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
                            to_execute.push(agent_id.clone());

                            // Schedule next execution
                            if let Some(interval) = task.interval_seconds {
                                task.next_execution = now + interval as i64;
                            } else if task.cron_expression.is_some() {
                                // For cron, just reschedule for next day (simplified)
                                task.next_execution = now + 86400;
                            } else {
                                // One-time execution, disable
                                task.enabled = false;
                            }
                        }
                    }

                    to_execute
                };

                // Execute tasks
                for agent_id in tasks_to_execute {
                    let executor = executor_ref.clone();
                    let running_executions_clone = running_executions.clone();

                    // Mark as running
                    running_executions_clone.write().await.insert(agent_id.clone());

                    tokio::spawn(async move {
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

        tracing::info!("Agent scheduler started");
        Ok(())
    }

    /// Stop the scheduler.
    pub async fn stop(&self) -> Result<(), crate::AgentError> {
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

    /// Calculate the next execution time for a schedule.
    pub(crate) fn calculate_next_execution(&self, schedule: &AgentSchedule) -> i64 {
        let now = chrono::Utc::now().timestamp();

        match schedule.schedule_type {
            ScheduleType::Interval => {
                if let Some(interval) = schedule.interval_seconds {
                    now + interval as i64
                } else {
                    now + 300 // Default 5 minutes
                }
            }
            ScheduleType::Cron => {
                // Simplified cron parsing - in production use a proper cron library
                // For now, just use a default interval
                now + 3600 // Default 1 hour
            }
            ScheduleType::Event => {
                // Event-triggered, no scheduled execution
                i64::MAX
            }
            ScheduleType::Once => {
                now // Execute immediately
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_calculate_next_execution() {
        let scheduler = AgentScheduler::new(SchedulerConfig::default()).await.unwrap();

        let interval_schedule = AgentSchedule {
            schedule_type: ScheduleType::Interval,
            cron_expression: None,
            interval_seconds: Some(300),
            event_filter: None,
            timezone: None,
        };

        let now = chrono::Utc::now().timestamp();
        let next = scheduler.calculate_next_execution(&interval_schedule);

        assert!(next >= now + 299 && next <= now + 301);
    }
}
