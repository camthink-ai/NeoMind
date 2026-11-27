//! Extension integration for rule engine.
//!
//! Provides extension command execution and value querying.

use async_trait::async_trait;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Default timeout applied to extension commands when none is explicitly set.
pub const DEFAULT_EXTENSION_TIMEOUT_MS: u64 = 30_000;

/// Extension command action for rules.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtensionCommandAction {
    pub extension_id: String,
    pub command: String,
    #[serde(default)]
    pub args: Value,
    #[serde(default)]
    pub timeout_ms: Option<u64>,
}

impl ExtensionCommandAction {
    pub fn new(extension_id: impl Into<String>, command: impl Into<String>) -> Self {
        Self {
            extension_id: extension_id.into(),
            command: command.into(),
            args: Value::Object(Default::default()),
            timeout_ms: None,
        }
    }

    pub fn with_args(mut self, args: impl Into<Value>) -> Self {
        self.args = args.into();
        self
    }

    pub fn with_timeout(mut self, timeout_ms: u64) -> Self {
        self.timeout_ms = Some(timeout_ms);
        self
    }
}

/// Extension registry trait — abstracted for dependency injection.
#[async_trait]
pub trait ExtensionRegistry: Send + Sync {
    async fn execute_command(
        &self,
        extension_id: &str,
        command: &str,
        args: &Value,
    ) -> Result<Value, String>;

    async fn has_extension(&self, extension_id: &str) -> bool;
}

/// Rule action executor for extension commands.
pub struct ExtensionActionExecutor {
    extension_registry: Arc<dyn ExtensionRegistry>,
}

impl ExtensionActionExecutor {
    pub fn new(registry: Arc<dyn ExtensionRegistry>) -> Self {
        Self {
            extension_registry: registry,
        }
    }

    pub async fn execute(
        &self,
        action: &ExtensionCommandAction,
    ) -> Result<ExecutionResult, String> {
        if !self
            .extension_registry
            .has_extension(&action.extension_id)
            .await
        {
            return Err(format!("Extension not found: {}", action.extension_id));
        }

        let start = std::time::Instant::now();

        let timeout_ms = action.timeout_ms.unwrap_or(DEFAULT_EXTENSION_TIMEOUT_MS);
        let timeout = tokio::time::Duration::from_millis(timeout_ms);
        let result = tokio::time::timeout(
            timeout,
            self.extension_registry.execute_command(
                &action.extension_id,
                &action.command,
                &action.args,
            ),
        )
        .await
        .map_err(|_| format!("Command timed out after {}ms", timeout_ms))??;

        let duration = start.elapsed();

        Ok(ExecutionResult {
            success: true,
            extension_id: action.extension_id.clone(),
            command: action.command.clone(),
            result,
            duration_ms: duration.as_millis() as u64,
            error: None,
        })
    }
}

/// Result of executing an extension command.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionResult {
    pub success: bool,
    pub extension_id: String,
    pub command: String,
    pub result: Value,
    pub duration_ms: u64,
    pub error: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extension_command_action_builder() {
        let action = ExtensionCommandAction::new("ext_id", "cmd_id")
            .with_args(serde_json::json!({"param1": 42}))
            .with_timeout(5000);

        assert_eq!(action.extension_id, "ext_id");
        assert_eq!(action.command, "cmd_id");
        assert_eq!(action.timeout_ms, Some(5000));
        assert_eq!(action.args["param1"], 42);
    }
}
