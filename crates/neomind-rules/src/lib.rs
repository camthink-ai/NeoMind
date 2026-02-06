//! Edge AI Rules Engine Crate
//!
//! This crate provides a rule engine with DSL support for the NeoMind platform.
//!
//! ## Features
//!
//! - **DSL Parser**: Human-readable rule definition language
//! - **Rule Engine**: Condition evaluation and action execution
//! - **Value Provider**: Integration with device metrics
//!
//! ## Example
//!
//! ```rust,no_run
//! use neomind_rules::{RuleEngine, RuleDslParser, InMemoryValueProvider};
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     let provider = std::sync::Arc::new(InMemoryValueProvider::new());
//!     let engine = RuleEngine::new(provider);
//!
//!     let dsl = r#"
//!         RULE "高温告警"
//!         WHEN sensor.temperature > 50
//!         DO
//!             NOTIFY "设备温度过高"
//!         END
//!     "#;
//!
//!     let rule_id = engine.add_rule_from_dsl(dsl).await?;
//!     println!("Added rule: {}", rule_id);
//!
//!     Ok(())
//! }
//! ```

pub mod dependencies;
pub mod device_integration;
pub mod dsl;
pub mod engine;
pub mod error;

pub mod history;
pub mod store;
pub mod validator;

pub use dependencies::{
    DependencyManager, DependencyType, DependencyValidationResult, RuleDependency,
};
pub use device_integration::{
    CommandActionResult, CommandExecutionStats, CommandResultHistory, CommandResultValue,
    DeviceActionExecutor, DeviceIntegratedRuleEngine, DeviceIntegrationError,
    DeviceIntegrationResult, DeviceValueProvider,
};
pub use dsl::{ComparisonOperator, LogLevel, ParsedRule, RuleAction, RuleCondition, RuleDslParser};
pub use engine::{
    CompiledRule, InMemoryValueProvider, RuleEngine, RuleExecutionResult, RuleId, RuleState,
    RuleStatus, ValueProvider,
};
pub use error::{NeoMindError, RuleError};

pub use history::{
    HistoryError, HistoryFilter, RuleHistoryEntry, RuleHistoryStats, RuleHistoryStorage,
};
pub use store::{
    ImportResult, RuleHistoryStats as StoreRuleHistoryStats, RuleStore, RuleStoreConfig,
    RulesExport, StoreError, ExportFormat,
};
pub use validator::{
    AlertChannelInfo, CommandInfo, DeviceInfo, MetricDataType, MetricInfo, ParameterInfo,
    RuleValidationResult, RuleValidator, ValidationContext, ValidationError, ValidationIssue,
    ValidationSeverity, ValidationResult, AvailableResources, ResourceSummary,
};

/// Version information
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_version() {
        assert!(!VERSION.is_empty());
    }
}
