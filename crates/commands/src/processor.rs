//! Command processor for handling command execution.
//!
//! Processes commands from the queue and sends them via downlink adapters.

use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;
use tokio::task::JoinHandle;

use crate::command::{CommandStatus, CommandResult};
use crate::queue::CommandQueue;
use crate::adapter::DownlinkAdapterRegistry;

/// Processor configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessorConfig {
    /// Poll interval for checking queue
    pub poll_interval_ms: u64,
    /// Maximum concurrent commands being processed
    pub max_concurrent: usize,
    /// Timeout for command execution
    pub default_timeout_secs: u64,
}

impl Default for ProcessorConfig {
    fn default() -> Self {
        Self {
            poll_interval_ms: 100,
            max_concurrent: 10,
            default_timeout_secs: 30,
        }
    }
}

/// Command processor.
pub struct CommandProcessor {
    config: ProcessorConfig,
    queue: Arc<CommandQueue>,
    adapters: Arc<RwLock<DownlinkAdapterRegistry>>,
    running: Arc<RwLock<bool>>,
    task_handle: Arc<RwLock<Option<JoinHandle<()>>>>,
}

impl CommandProcessor {
    /// Create a new command processor.
    pub fn new(
        queue: Arc<CommandQueue>,
        adapters: Arc<RwLock<DownlinkAdapterRegistry>>,
        config: ProcessorConfig,
    ) -> Self {
        Self {
            config,
            queue,
            adapters,
            running: Arc::new(RwLock::new(false)),
            task_handle: Arc::new(RwLock::new(None)),
        }
    }

    /// Start the processor.
    pub async fn start(&self) {
        let mut running = self.running.write().await;
        if *running {
            return;
        }
        *running = true;
        drop(running);

        let queue = self.queue.clone();
        let adapters = self.adapters.clone();
        let running_flag = self.running.clone();
        let poll_interval = Duration::from_millis(self.config.poll_interval_ms);

        let handle = tokio::spawn(async move {
            let mut interval = tokio::time::interval(poll_interval);

            loop {
                // Check if still running
                {
                    let r = running_flag.read().await;
                    if !*r {
                        break;
                    }
                }

                interval.tick().await;

                // Try to get next command
                if let Some(mut command) = queue.try_dequeue().await {
                    command.update_status(CommandStatus::Sending);

                    // Get appropriate adapter
                    let _adapters_ref = adapters.read().await;
                    // In a real implementation, we'd look up the adapter by device type
                    // For now, just mark as completed
                    command.update_status(CommandStatus::Completed);
                    command.set_result(CommandResult::success("Command sent"));

                    // Store result (would use command state store here)
                }
            }
        });

        let mut task = self.task_handle.write().await;
        *task = Some(handle);
    }

    /// Stop the processor.
    pub async fn stop(&self) {
        let mut running = self.running.write().await;
        *running = false;
        drop(running);

        // Wait for task to complete
        let mut task = self.task_handle.write().await;
        if let Some(handle) = task.take() {
            drop(task);
            handle.await.ok();
        }
    }

    /// Check if processor is running.
    pub async fn is_running(&self) -> bool {
        *self.running.read().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_processor_config_default() {
        let config = ProcessorConfig::default();
        assert_eq!(config.poll_interval_ms, 100);
        assert_eq!(config.max_concurrent, 10);
        assert_eq!(config.default_timeout_secs, 30);
    }
}
