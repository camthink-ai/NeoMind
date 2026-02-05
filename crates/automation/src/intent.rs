//! Intent analysis for determining automation type from natural language
//!
//! This module uses LLM analysis to determine whether a user's description
//! is better suited for a Transform (data processing) or a Rule (conditional automation).

use std::sync::Arc;

use crate::types::*;
use crate::error::{AutomationError, Result};

use edge_ai_core::{LlmRuntime, Message, GenerationParams};
use edge_ai_core::llm::backend::LlmInput;

/// Intent analyzer for determining automation type
pub struct IntentAnalyzer {
    llm: Arc<dyn LlmRuntime>,
}

impl IntentAnalyzer {
    /// Create a new intent analyzer
    pub fn new(llm: Arc<dyn LlmRuntime>) -> Self {
        Self { llm }
    }

    /// Analyze a natural language description to determine automation type
    pub async fn analyze(&self, description: &str) -> Result<IntentResult> {
        let description = description.trim();
        if description.is_empty() {
            return Ok(IntentResult {
                recommended_type: AutomationType::Rule,
                confidence: 0,
                reasoning: "Empty description".to_string(),
                suggested_automation: None,
                warnings: vec!["Description is empty".to_string()],
            });
        }

        // First, do heuristic analysis for quick response
        let heuristic = self.heuristic_analysis(description);

        // If confidence is high from heuristics, use that
        if heuristic.confidence >= 80 {
            return Ok(heuristic);
        }

        // Otherwise, use LLM for deeper analysis
        self.llm_analysis(description).await
    }

    /// Quick heuristic analysis without LLM
    fn heuristic_analysis(&self, description: &str) -> IntentResult {
        let desc_lower = description.to_lowercase();

        // Indicators for transform (data processing)
        let transform_keywords = [
            "calculate", "compute", "aggregate", "average", "sum", "count",
            "extract", "parse", "transform", "convert", "process",
            "statistics", "metric", "virtual", "derived",
            "array", "group by", "filter", "map",
            "data from", "get value", "field",
        ];

        // Indicators for rule (conditional automation)
        let rule_keywords = [
            "when", "if", "whenever", "once",
            "exceeds", "above", "below", "greater", "less",
            "equals", "matches", "is",
            "send alert", "notify", "send message",
            "turn on", "turn off", "switch", "set",
            "trigger", "activate", "deactivate",
            "then", "after that",
        ];

        let mut transform_score = 0i32;
        let mut rule_score = 0i32;

        // Count transform indicators
        for keyword in &transform_keywords {
            if desc_lower.contains(keyword) {
                transform_score += 5;
            }
        }

        // Count rule indicators
        for keyword in &rule_keywords {
            if desc_lower.contains(keyword) {
                rule_score += 5;
            }
        }

        // Check for device/action patterns (suggests rule)
        if desc_lower.contains("device") || desc_lower.contains("send") {
            rule_score += 10;
        }

        // Check for data processing patterns (suggests transform)
        if desc_lower.contains("data") || desc_lower.contains("value") {
            transform_score += 5;
        }

        // Determine result
        let (recommended_type, confidence, reasoning) = if transform_score > rule_score + 10 {
            (
                AutomationType::Transform,
                (transform_score - rule_score).min(100) as u8,
                format!(
                    "This appears to be a data processing automation with {} transform indicators",
                    transform_score / 5
                ),
            )
        } else if rule_score > transform_score + 10 {
            (
                AutomationType::Rule,
                (rule_score - transform_score).min(100) as u8,
                "This appears to be a conditional automation".to_string(),
            )
        } else {
            // Uncertain - default to rule for automation use cases
            (
                AutomationType::Rule,
                55,
                "This could be either a transform or rule. Defaulting to rule for automation.".to_string(),
            )
        };

        IntentResult {
            recommended_type,
            confidence,
            reasoning,
            suggested_automation: None,
            warnings: if confidence < 60 {
                vec!["Low confidence - consider reviewing the automation type".to_string()]
            } else {
                Vec::new()
            },
        }
    }

    /// LLM-powered analysis for complex cases
    async fn llm_analysis(&self, description: &str) -> Result<IntentResult> {
        let prompt = format!(
            r#"Analyze the following automation description and determine whether it's better implemented as a Transform (data processing) or a Rule (conditional automation).

Description: "{}"

Respond in JSON format:
{{
  "recommended_type": "transform" | "rule",
  "confidence": 0-100,
  "reasoning": "Brief explanation",
  "suggested_name": "a clear name for this automation",
  "suggested_description": "a clear description"
}}

Guidelines:
- Recommend "transform" for: data processing, calculations, aggregations, extracting values from device data
- Recommend "rule" for: condition-based actions, sending alerts, device commands, reactive automation
- Confidence should reflect how clear-cut the choice is"#,
            description
        );

        let input = LlmInput {
            messages: vec![Message::user(prompt)],
            params: GenerationParams {
                temperature: Some(0.3),
                max_tokens: Some(500),
                ..Default::default()
            },
            model: None,
            stream: false,
            tools: None,
        };

        let response = self.llm.generate(input).await?;

        // Parse the JSON response
        let json_str = extract_json_from_response(&response.text)?;
        let analysis: serde_json::Value = serde_json::from_str(&json_str)
            .map_err(|e| AutomationError::IntentAnalysisFailed(format!("Invalid JSON: {}", e)))?;

        let recommended_type = match analysis["recommended_type"].as_str() {
            Some("transform") => AutomationType::Transform,
            _ => AutomationType::Rule,
        };

        let confidence = analysis["confidence"].as_u64().unwrap_or(50) as u8;
        let reasoning = analysis["reasoning"].as_str()
            .unwrap_or("AI analysis completed")
            .to_string();

        let suggested_name = analysis["suggested_name"].as_str()
            .unwrap_or("New Automation")
            .to_string();

        let suggested_description = analysis["suggested_description"].as_str()
            .unwrap_or(description)
            .to_string();

        Ok(IntentResult {
            recommended_type,
            confidence: confidence.min(100),
            reasoning,
            suggested_automation: Some(SuggestedAutomation {
                name: suggested_name,
                description: suggested_description,
                automation_type: recommended_type,
                transform: None,
                rule: None,
                estimated_complexity: if recommended_type == AutomationType::Transform { 2 } else { 1 },
            }),
            warnings: Vec::new(),
        })
    }
}

/// Extract JSON from an LLM response that might have extra text
fn extract_json_from_response(response: &str) -> Result<String> {
    // Find the first { and last }
    let start = response.find('{').ok_or_else(|| {
        AutomationError::IntentAnalysisFailed("No JSON object found in response".to_string())
    })?;

    let end = response.rfind('}').ok_or_else(|| {
        AutomationError::IntentAnalysisFailed("Incomplete JSON object".to_string())
    })?;

    Ok(response[start..=end].to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use edge_ai_core::*;
    use edge_ai_core::llm::backend::{LlmInput, LlmError, StreamChunk};
    use std::pin::Pin;
    use std::result::Result;
    use futures::Stream;

    // Mock LLM for testing
    struct MockLlm;

    #[async_trait::async_trait]
    impl LlmRuntime for MockLlm {
        fn backend_id(&self) -> BackendId {
            BackendId::new("mock")
        }

        fn model_name(&self) -> &str {
            "mock-model"
        }

        fn max_context_length(&self) -> usize {
            4096
        }

        fn capabilities(&self) -> BackendCapabilities {
            BackendCapabilities::default()
        }

        async fn generate(&self, _input: LlmInput) -> Result<LlmOutput, LlmError> {
            Ok(LlmOutput {
                text: r#"{"recommended_type": "rule", "confidence": 90, "reasoning": "Simple condition"}"#.to_string(),
                finish_reason: FinishReason::Stop,
                usage: None,
                thinking: None,
            })
        }

        async fn generate_stream(&self, _input: LlmInput) -> Result<Pin<Box<dyn Stream<Item = StreamChunk> + Send>>, LlmError> {
            use futures::stream;
            Ok(Box::pin(stream::empty()))
        }
    }

    #[test]
    fn test_heuristic_rule() {
        let analyzer = IntentAnalyzer {
            llm: Arc::new(MockLlm),
        };

        let result = analyzer.heuristic_analysis("When temperature exceeds 30, send an alert");
        assert_eq!(result.recommended_type, AutomationType::Rule);
    }

    #[test]
    fn test_heuristic_transform() {
        let analyzer = IntentAnalyzer {
            llm: Arc::new(MockLlm),
        };

        let result = analyzer.heuristic_analysis("Calculate average temperature from sensor array");
        assert_eq!(result.recommended_type, AutomationType::Transform);
    }

    #[test]
    fn test_extract_json() {
        let response = r#"Here's my analysis:
        {
          "recommended_type": "rule",
          "confidence": 90
        }
        Thanks!"#;

        let json = extract_json_from_response(response).unwrap();
        assert!(json.contains("recommended_type"));
    }
}
