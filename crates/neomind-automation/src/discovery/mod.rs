//! Device data discovery and AI-powered analysis module.
//!
//! This module provides capabilities for:
//! - Extracting data paths from device samples
//! - Inferring semantic meaning of fields
//! - Auto-generating virtual metrics
//! - Generating device type definitions
//! - Zero-config auto-onboarding of unknown devices
//!
//! ## Analysis Pipeline
//!
//! The discovery system uses a hybrid approach with deterministic and AI-based analysis:
//!
//! 1. **Structure Analysis** - Fast, deterministic structure extraction
//! 2. **Statistics Analysis** - Value distribution and pattern detection
//! 3. **Hex Analysis** - Hex-encoded data detection and interpretation
//! 4. **Semantic Inference** - AI-powered semantic understanding (with fallback)

pub mod auto_onboard;
pub mod hex_analyzer;
pub mod path_extractor;
pub mod semantic_inference;
pub mod statistics_analyzer;
pub mod structure_analyzer;
pub mod types;
pub mod virtual_metric;

pub use auto_onboard::{AutoOnboardManager, RegistrationResult, TypeSignature};
pub use hex_analyzer::{
    compute_stats as hex_compute_stats, hex_to_bytes, is_hex_string, HexAnalyzer, HexInfo,
    HexProbability, MetricInterpretation, SuggestedType,
};
pub use path_extractor::DataPathExtractor;
pub use semantic_inference::{MetricEnhancement, SemanticInference};
pub use statistics_analyzer::{
    compute_quick_stats, StatisticsAnalyzer, StatisticsResult, ValuePattern, ValueStatistics,
};
pub use structure_analyzer::{
    extract_field_name, normalize_path, InferredType, PathInfo, StructureAnalyzer, StructureResult,
};
pub use types::*;
pub use virtual_metric::{
    AggregationOperation, AggregationSuggestion, DerivedMetric, VirtualMetricGenerator,
};
