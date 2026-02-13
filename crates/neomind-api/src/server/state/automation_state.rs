//! Automation and rules state.
//!
//! Contains all automation-related services:
//! - RuleEngine for DSL rule evaluation
//! - RuleStore for persistent rule storage
//! - AutomationStore for unified automations
//! - IntentAnalyzer for automation type recommendations
//! - TransformEngine for data processing

use std::sync::Arc;

use neomind_automation::{
    intent::IntentAnalyzer, store::SharedAutomationStore, transform::TransformEngine,
};
use neomind_rules::{store::RuleStore, RuleEngine};
use neomind_storage::business::RuleHistoryStore;

/// Automation and rules state.
///
/// Provides access to rule engine, automation stores, and related services.
#[derive(Clone)]
pub struct AutomationState {
    /// Rule engine for DSL rule evaluation.
    pub rule_engine: Arc<RuleEngine>,

    /// Rule store for persistent rule storage.
    pub rule_store: Option<Arc<RuleStore>>,

    /// Automation store for unified automations (rules + transforms).
    pub automation_store: Option<Arc<SharedAutomationStore>>,

    /// Intent analyzer for automation type recommendations (lazy-initialized).
    pub intent_analyzer: Option<Arc<IntentAnalyzer>>,

    /// Transform engine for data processing.
    pub transform_engine: Option<Arc<TransformEngine>>,

    /// Rule history store for statistics.
    pub rule_history_store: Option<Arc<RuleHistoryStore>>,
}

impl AutomationState {
    /// Create a new automation state.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        rule_engine: Arc<RuleEngine>,
        rule_store: Option<Arc<RuleStore>>,
        automation_store: Option<Arc<SharedAutomationStore>>,
        intent_analyzer: Option<Arc<IntentAnalyzer>>,
        transform_engine: Option<Arc<TransformEngine>>,
        rule_history_store: Option<Arc<RuleHistoryStore>>,
    ) -> Self {
        Self {
            rule_engine,
            rule_store,
            automation_store,
            intent_analyzer,
            transform_engine,
            rule_history_store,
        }
    }

    /// Create a minimal automation state (for testing).
    #[cfg(test)]
    pub fn minimal() -> Self {
        use neomind_rules::InMemoryValueProvider;
        Self {
            rule_engine: Arc::new(RuleEngine::new(Arc::new(InMemoryValueProvider::new()))),
            rule_store: None,
            automation_store: None,
            intent_analyzer: None,
            transform_engine: None,
            rule_history_store: None,
        }
    }
}
