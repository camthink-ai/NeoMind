//! Edge AI Unified Automation Crate
//!
//! This crate provides a unified abstraction for transforms and rules in the NeoTalk platform.
//!
//! ## Features
//!
//! - **Transform Layer**: Process raw device data into usable metrics
//! - **Unified Types**: Single `Automation` enum that wraps transforms and rules
//! - **Intent Analysis**: AI-powered recommendation of transform/rule based on natural language
//! - **Shared Resources**: Common templates, devices, and metrics for all types
//!
//! ## Example
//!
//! ```rust,no_run,ignore
//! use edge_ai_automation::{Automation, AutomationType, TransformScope, TransformOperation};
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     // Create a transform to process sensor array data
//!     let transform = TransformAutomation::new("avg-temp", "Average Temperature", TransformScope::DeviceType("sensor".to_string()))
//!         .with_operation(TransformOperation::ArrayAggregation {
//!             json_path: "$.sensors",
//!             aggregation: AggregationFunc::Mean,
//!             value_path: Some("temp".to_string()),
//!             output_metric: "temperature_avg".to_string(),
//!         });
//!
//!     // Analyze intent to determine automation type
//!     let intent = analyzer.analyze("When temperature exceeds 30°C, send an alert").await?;
//!
//!     Ok(())
//! }
//! ```

pub mod conversion;
pub mod discovery;
pub mod device_type_generator;
pub mod error;
pub mod intent;
pub mod nl2automation;
pub mod threshold_recommender;
pub mod transform;
pub mod types;
pub mod store;

// Re-export all types
pub use types::*;

// Re-export core types with common aliases
pub use error::{AutomationError, Result};
pub use conversion::AutomationConverter;
pub use nl2automation::{Nl2Automation, ExtractedEntities, TriggerEntity, ConditionEntity, ActionEntity, TriggerTypeEntity, ActionTypeEntity, TimeConstraints};
pub use threshold_recommender::{ThresholdRecommender, ThresholdRequest, ThresholdRecommendation, ThresholdIntent, ThresholdValidation, Statistics};
pub use device_type_generator::{DeviceTypeGenerator, DeviceCapabilities, ValidationResult, GenerationConfig};

// Re-export discovery module
pub use discovery::{
    DataPathExtractor,
    DeviceSample,
    DiscoveredPath,
    SemanticType,
    SemanticInference,
    DiscoveredMetric,
    FieldSemantic,
    InferenceContext,
    VirtualMetricGenerator,
    AutoOnboardManager,
    RegistrationResult,
    TypeSignature,
    DraftDevice,
    DraftDeviceStatus,
    GeneratedDeviceType,
    ProcessingSummary,
    DeviceCategory,
};

// Re-export transform-specific types
pub use types::{
    TransformAutomation, TransformOperation, TransformScope,
    AggregationFunc, TimeWindow,
    RuleAutomation,
    Automation, AutomationType, AutomationMetadata,
    Trigger, TriggerType, Condition, ComparisonOperator,
    Action, LogLevel, AlertSeverity,
    IntentResult, SuggestedAutomation,
    AutomationTemplate, TemplateParameter, TemplateParameterType,
    ExecutionRecord, ExecutionStatus,
    AutomationFilter, AutomationList, TypeCounts,
};

// Re-export transform engine
pub use transform::{TransformEngine, TransformResult, TransformedMetric};
