//! Edge AI Rules Engine Crate
//!
//! This crate provides a rule engine with DSL support for the NeoTalk platform.
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
//! use edge_ai_rules::{RuleEngine, RuleDslParser, InMemoryValueProvider};
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

pub mod dsl;
pub mod engine;
pub mod error;
pub mod integration;
pub mod device_integration;
pub mod history;

pub use dsl::{RuleDslParser, ParsedRule, RuleCondition, RuleAction, ComparisonOperator, LogLevel};
pub use engine::{RuleEngine, CompiledRule, RuleId, RuleStatus, RuleState, RuleExecutionResult, ValueProvider, InMemoryValueProvider};
pub use error::{RuleError, NeoTalkError};
pub use integration::{EventDrivenRuleEngine, EventEngineResult, EventEngineError, CachedValueProvider};
pub use device_integration::{DeviceValueProvider, DeviceActionExecutor, DeviceIntegratedRuleEngine, DeviceIntegrationResult, DeviceIntegrationError};
pub use history::{RuleHistoryStorage, RuleHistoryEntry, RuleHistoryStats, HistoryFilter, HistoryError};

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
