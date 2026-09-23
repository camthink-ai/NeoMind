//! Rule engine v2 — event-driven, pure JSON rules.
//!
//! ## Quick start
//!
//! ```rust,no_run
//! use neomind_rules::{RuleEngine, CompiledRule};
//! use neomind_rules::models::*;
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     let provider = std::sync::Arc::new(neomind_rules::InMemoryValueProvider::new());
//!     let engine = RuleEngine::new(provider);
//!
//!     let mut rule = CompiledRule::new("High Temperature");
//!     rule.condition = Some(RuleCondition::Comparison {
//!         source: neomind_core::datasource::DataSourceId::device("sensor1", "temperature"),
//!         operator: ComparisonOperator::GreaterThan,
//!         threshold: 50.0,
//!         threshold_value: None,
//!     });
//!     rule.trigger = RuleTrigger::from_condition(&rule.condition);
//!     rule.actions = vec![RuleAction::Notify {
//!         message: "Too hot!".into(),
//!         severity: NotifySeverity::Critical,
//!     }];
//!     rule.finalize();
//!
//!     engine.add_rule(rule).await?;
//!     Ok(())
//! }
//! ```

pub mod device_integration;
pub mod device_status_emitter;
pub mod chain;
pub mod engine;
pub mod error;
pub mod extension_integration;
pub mod models;
pub mod preview;
pub mod store;
pub mod unified_provider;
pub mod validator;

// Re-exports
pub use device_status_emitter::{
    DeviceStatusEmitter, VIRTUAL_METRIC_NAME as DEVICE_LAST_SEEN_AGE_METRIC,
};
pub use engine::{AgentTriggerCallback, InMemoryValueProvider, RuleEngine};
pub use error::RuleError;
pub use models::{
    ComparisonOperator, CompiledRule, ExecuteTarget, LogicalOperator, NotifySeverity, RuleAction,
    RuleCondition, RuleExecutionResult, RuleId, RuleState, RuleTrigger, RuleValue, ValueProvider,
};
pub use preview::to_dsl_preview;
pub use unified_provider::UnifiedValueProvider;
pub use validator::{
    AlertChannelInfo, CommandInfo, DeviceInfo, MetricDataType, MetricInfo, ParameterInfo,
    RuleValidator, ValidationContext, VIRTUAL_METRICS,
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
