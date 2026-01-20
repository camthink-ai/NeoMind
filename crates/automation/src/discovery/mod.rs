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

pub mod types;
pub mod path_extractor;
pub mod semantic_inference;
pub mod virtual_metric;
pub mod auto_onboard;
pub mod structure_analyzer;
pub mod statistics_analyzer;
pub mod hex_analyzer;

pub use types::*;
pub use path_extractor::DataPathExtractor;
pub use semantic_inference::{SemanticInference, MetricEnhancement};
pub use virtual_metric::{VirtualMetricGenerator, AggregationSuggestion, AggregationOperation, DerivedMetric};
pub use auto_onboard::{AutoOnboardManager, TypeSignature, RegistrationResult};
pub use structure_analyzer::{StructureAnalyzer, StructureResult, PathInfo, InferredType, normalize_path, extract_field_name};
pub use statistics_analyzer::{StatisticsAnalyzer, StatisticsResult, ValueStatistics, ValuePattern, compute_quick_stats};
pub use hex_analyzer::{HexAnalyzer, HexInfo, HexProbability, MetricInterpretation, SuggestedType, hex_to_bytes, is_hex_string, compute_stats as hex_compute_stats};
