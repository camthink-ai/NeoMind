//! Analysis tools for trend analysis, anomaly detection, and decision making.
//!
//! This module provides tools that the LLM can use to analyze system data,
//! detect anomalies, and propose decisions.

pub mod anomalies;
pub mod trends;

pub use anomalies::DetectAnomaliesTool;
pub use trends::AnalyzeTrendsTool;
