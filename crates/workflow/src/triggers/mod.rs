//! Workflow trigger implementations.
//!
//! This module contains various trigger types for workflows.

pub mod event;
pub mod llm_decision;

pub use event::{EventTrigger, EventTriggerConfig, EventFilters, EventTriggerManager};
pub use llm_decision::{
    LlmDecisionTrigger, LlmDecisionTriggerConfig, LlmDecisionTriggerManager,
};
