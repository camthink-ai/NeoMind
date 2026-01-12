//! Autonomous Agent framework for LLM-driven decision making.
//!
//! This module provides the autonomous agent that periodically reviews
//! system state and generates decision proposals using LLM.

pub mod agent;
pub mod config;
pub mod review;
pub mod context;
pub mod decision;

pub use agent::{AutonomousAgent, AgentState};
pub use config::{AutonomousConfig, ReviewType};
pub use review::{ReviewContext, ReviewResult, SystemReview};
pub use context::{
    SystemContext,
    ContextCollector,
    TimeRange,
    EnergyData,
    MetricAggregation,
};
pub use decision::{
    Decision,
    DecisionAction,
    DecisionType,
    DecisionPriority,
    DecisionStatus,
    DecisionEngine,
    DecisionEngineConfig,
    ImpactAssessment,
    DecisionError,
};
