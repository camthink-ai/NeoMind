//! Analysis tools for trend analysis, anomaly detection, and decision making.
//!
//! This module provides tools that the LLM can use to analyze system data,
//! detect anomalies, propose decisions, and execute actions.

pub mod trends;
pub mod anomalies;
pub mod decisions;

pub use trends::AnalyzeTrendsTool;
pub use anomalies::DetectAnomaliesTool;
pub use decisions::{ProposeDecisionTool, ExecuteDecisionTool};
