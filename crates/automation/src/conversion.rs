//! Conversion between automation types
//!
//! This module provides utilities for conversion recommendations between
//! transform and rule automations.

use crate::types::*;

/// Converter for analyzing automation conversion options
pub struct AutomationConverter;

impl AutomationConverter {

    /// Get conversion recommendations for an automation
    pub fn get_conversion_recommendation(automation: &Automation) -> ConversionRecommendation {
        match automation {
            Automation::Transform(transform) => {
                // Transforms cannot be directly converted to rules (different purposes)
                ConversionRecommendation {
                    can_convert: false,
                    target_type: AutomationType::Rule,
                    reason: "Transforms are for data processing and cannot be converted to rules. Consider creating a rule that uses the transform's output metrics.".to_string(),
                    estimated_complexity: transform.complexity_score(),
                }
            }
            Automation::Rule(_rule) => {
                // Rules cannot be converted to transforms (different purposes)
                ConversionRecommendation {
                    can_convert: false,
                    target_type: AutomationType::Transform,
                    reason: "Rules are for reactive automation and cannot be converted to transforms. Consider creating a transform if you need data processing.".to_string(),
                    estimated_complexity: 1,
                }
            }
        }
    }
}

/// Recommendation for type conversion
#[derive(Debug, Clone)]
pub struct ConversionRecommendation {
    /// Whether conversion is possible
    pub can_convert: bool,
    /// The target type for conversion
    pub target_type: AutomationType,
    /// Reason for the recommendation
    pub reason: String,
    /// Estimated complexity after conversion
    pub estimated_complexity: u8,
}
