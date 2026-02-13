//! Model capability detection module.
//!
//! This module provides functionality to detect and query model capabilities,
//! including support for streaming, function calling, vision, audio, reasoning, etc.

use crate::llm::models::{get_model_info, ModelCapabilities};

/// Result of capability detection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapabilityDetectionResult {
    /// Model name
    pub model: String,
    /// Provider type
    pub provider: crate::llm::models::ProviderType,
    /// Detected capabilities
    pub capabilities: ModelCapabilities,
    /// Whether detection was from built-in registry
    pub from_registry: bool,
}

/// Capability detector for LLM models.
pub struct CapabilityDetector {
    /// Cache of detected capabilities
    cache: std::collections::HashMap<String, CapabilityDetectionResult>,
}

impl CapabilityDetector {
    /// Create a new capability detector.
    pub fn new() -> Self {
        Self {
            cache: std::collections::HashMap::new(),
        }
    }

    /// Detect capabilities for a model.
    ///
    /// First checks the built-in model registry, then falls back to
    /// heuristic detection based on model name patterns.
    pub fn detect(&mut self, model: &str) -> Option<CapabilityDetectionResult> {
        // Check cache first
        if let Some(cached) = self.cache.get(model) {
            return Some(cached.clone());
        }

        // Try built-in registry first
        if let Some(info) = get_model_info(model) {
            let result = CapabilityDetectionResult {
                model: model.to_string(),
                provider: info.provider,
                capabilities: info.capabilities.clone(),
                from_registry: true,
            };
            self.cache.insert(model.to_string(), result.clone());
            return Some(result);
        }

        // Fallback to heuristic detection
        let result = self.heuristic_detect(model)?;
        self.cache.insert(model.to_string(), result.clone());
        Some(result)
    }

    /// Heuristic capability detection based on model name patterns.
    ///
    /// This is used when the model is not in the built-in registry.
    fn heuristic_detect(&self, model: &str) -> Option<CapabilityDetectionResult> {
        let model_lower = model.to_lowercase();

        // Detect provider from model name
        let provider = self.detect_provider(&model_lower);

        // Detect capabilities from model name patterns
        let capabilities = ModelCapabilities {
            streaming: self.detect_streaming(&model_lower),
            function_calling: self.detect_function_calling(&model_lower),
            vision: self.detect_vision(&model_lower),
            audio: self.detect_audio(&model_lower),
            video: self.detect_video(&model_lower),
            reasoning: self.detect_reasoning(&model_lower),
            max_context: Some(self.estimate_max_context(&model_lower)),
            json_mode: self.detect_json_mode(&model_lower),
        };

        Some(CapabilityDetectionResult {
            model: model.to_string(),
            provider,
            capabilities,
            from_registry: false,
        })
    }

    /// Detect provider from model name.
    fn detect_provider(&self, model: &str) -> crate::llm::models::ProviderType {
        use crate::llm::models::ProviderType;

        if model.contains("gpt") || model.contains("o1") || model.contains("o3") {
            ProviderType::OpenAI
        } else if model.contains("claude") {
            ProviderType::Anthropic
        } else if model.contains("gemini") {
            ProviderType::Google
        } else if model.contains("grok") {
            ProviderType::XAi
        } else if model.contains("qwen") {
            ProviderType::Qwen
        } else if model.contains("deepseek") {
            ProviderType::DeepSeek
        } else if model.contains("glm") || model.contains("zhipu") {
            ProviderType::GLM
        } else if model.contains("minimax") || model.contains("m2-") || model.contains("abab") {
            ProviderType::MiniMax
        } else if model.contains("llama") || model.contains("mistral") {
            ProviderType::Ollama
        } else {
            ProviderType::Custom
        }
    }

    /// Detect streaming capability (most modern models support it).
    fn detect_streaming(&self, model: &str) -> bool {
        // Most modern LLMs support streaming
        // Older models or specific variants might not
        !model.contains("-legacy") && !model.contains("-v1-")
    }

    /// Detect function calling capability.
    fn detect_function_calling(&self, model: &str) -> bool {
        // GPT-4 and later, Claude 3+, Gemini, etc. support function calling
        model.contains("gpt-4")
            || model.contains("gpt-4o")
            || model.contains("claude-3")
            || model.contains("gemini-1.5")
            || model.contains("gemini-2.")
            || model.contains("qwen-max")
            || model.contains("qwen-plus")
            || model.contains("deepseek")
            || model.contains("glm-4")
            || model.contains("glm-5")
            || model.contains("minimax")
            || model.contains("grok")
            || model.contains("llama-3.1")
            || model.contains("llama-3.2")
            || model.contains("llama-3.3")
    }

    /// Detect vision/multimodal capability.
    fn detect_vision(&self, model: &str) -> bool {
        model.contains("vision")
            || model.contains("vl")
            || model.contains("-4o") // GPT-4o
            || model.contains("claude-3")
            || model.contains("gemini")
            || model.contains("qwen-vl")
            || model.contains("qwen-omni")
            || model.contains("deepseek-v3") // DeepSeek v3 has vision
            || model.contains("glm-4v")
            || model.contains("minimax-vl")
            || model.contains("m2-her")
    }

    /// Detect audio capability.
    fn detect_audio(&self, model: &str) -> bool {
        model.contains("audio")
            || model.contains("tts")
            || model.contains("asr")
            || model.contains("whisper")
            || model.contains("gpt-4o")
            || model.contains("qwen-audio")
            || model.contains("qwen-tts")
            || model.contains("qwen-omni")
            || model.contains("minimax-speech")
    }

    /// Detect video capability.
    fn detect_video(&self, model: &str) -> bool {
        model.contains("video") || model.contains("qwen-video") || model.contains("qwen-omni")
    }

    /// Detect reasoning capability (o1, o3, deepseek-r1, etc).
    fn detect_reasoning(&self, model: &str) -> bool {
        model.contains("o1")
            || model.contains("o3")
            || model.contains("r1")
            || model.contains("reasoning")
            || model.contains("qwq") // Qwen reasoning models
            || model.contains("glm-z1")
    }

    /// Estimate max context length based on model name.
    fn estimate_max_context(&self, model: &str) -> usize {
        if model.contains("gpt-4") || model.contains("o1") || model.contains("o3") {
            if model.contains("turbo") {
                128000
            } else if model.contains("o1") || model.contains("o3") {
                200000
            } else {
                128000
            }
        } else if model.contains("claude") {
            if model.contains("claude-3") {
                200000
            } else {
                100000
            }
        } else if model.contains("gemini-2.") {
            1000000
        } else if model.contains("gemini-1.5") {
            if model.contains("pro") {
                1000000
            } else {
                1000000
            }
        } else if model.contains("qwen") {
            if model.contains("qwen-long") {
                1000000
            } else if model.contains("qwen-max") || model.contains("qwen-plus") {
                128000
            } else if model.contains("qwen-vl") {
                32768
            } else {
                32768
            }
        } else if model.contains("deepseek") {
            if model.contains("deepseek-r1") {
                64000
            } else if model.contains("deepseek-v3") {
                128000
            } else {
                128000
            }
        } else if model.contains("glm") {
            if model.contains("glm-5") {
                1000000
            } else if model.contains("glm-4-plus") || model.contains("glm-4-air") {
                128000
            } else if model.contains("glm-4-flash") {
                128000
            } else {
                128000
            }
        } else if model.contains("minimax") {
            if model.contains("m2-1") || model.contains("m2-her") {
                512000
            } else {
                245760
            }
        } else {
            // Default conservative estimate
            8192
        }
    }

    /// Detect JSON mode capability.
    fn detect_json_mode(&self, model: &str) -> bool {
        model.contains("gpt-4")
            || model.contains("gpt-4o")
            || model.contains("claude-3")
            || model.contains("gemini")
            || model.contains("qwen-max")
            || model.contains("qwen-plus")
            || model.contains("deepseek")
            || model.contains("glm-4")
            || model.contains("glm-5")
            || model.contains("minimax")
            || model.contains("grok")
            || model.contains("llama-3.1")
            || model.contains("llama-3.2")
            || model.contains("llama-3.3")
    }

    /// Get all cached capabilities.
    pub fn get_all_cached(&self) -> Vec<CapabilityDetectionResult> {
        self.cache.values().cloned().collect()
    }

    /// Clear the capability cache.
    pub fn clear_cache(&mut self) {
        self.cache.clear();
    }
}

impl Default for CapabilityDetector {
    fn default() -> Self {
        Self::new()
    }
}

/// Check if a model supports a specific capability.
///
/// This is a convenience function that creates a detector
/// and checks a single capability.
pub fn model_supports(model: &str, capability: &str) -> bool {
    let mut detector = CapabilityDetector::new();

    match capability {
        "streaming" => detector
            .detect(model)
            .map(|r| r.capabilities.streaming)
            .unwrap_or(false),
        "function_calling" | "tools" => detector
            .detect(model)
            .map(|r| r.capabilities.function_calling)
            .unwrap_or(false),
        "vision" | "vl" | "multimodal" => detector
            .detect(model)
            .map(|r| r.capabilities.vision)
            .unwrap_or(false),
        "audio" => detector
            .detect(model)
            .map(|r| r.capabilities.audio)
            .unwrap_or(false),
        "video" => detector
            .detect(model)
            .map(|r| r.capabilities.video)
            .unwrap_or(false),
        "reasoning" => detector
            .detect(model)
            .map(|r| r.capabilities.reasoning)
            .unwrap_or(false),
        "json" | "json_mode" => detector
            .detect(model)
            .map(|r| r.capabilities.json_mode)
            .unwrap_or(false),
        _ => false,
    }
}

/// Get the max context length for a model.
pub fn get_max_context(model: &str) -> usize {
    let mut detector = CapabilityDetector::new();
    detector
        .detect(model)
        .and_then(|r| r.capabilities.max_context)
        .unwrap_or(8192)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_provider() {
        let detector = CapabilityDetector::new();

        assert_eq!(
            detector.detect_provider("gpt-4o"),
            crate::llm::models::ProviderType::OpenAI
        );
        assert_eq!(
            detector.detect_provider("claude-3-5-sonnet"),
            crate::llm::models::ProviderType::Anthropic
        );
        assert_eq!(
            detector.detect_provider("gemini-2.0-flash"),
            crate::llm::models::ProviderType::Google
        );
        assert_eq!(
            detector.detect_provider("qwen-max-latest"),
            crate::llm::models::ProviderType::Qwen
        );
        assert_eq!(
            detector.detect_provider("deepseek-v3"),
            crate::llm::models::ProviderType::DeepSeek
        );
        assert_eq!(
            detector.detect_provider("glm-4-plus"),
            crate::llm::models::ProviderType::GLM
        );
        assert_eq!(
            detector.detect_provider("minimax-text-01"),
            crate::llm::models::ProviderType::MiniMax
        );
    }

    #[test]
    fn test_detect_vision() {
        let detector = CapabilityDetector::new();

        assert!(detector.detect_vision("gpt-4o"));
        assert!(detector.detect_vision("qwen-vl-max"));
        assert!(detector.detect_vision("claude-3-5-sonnet"));
        assert!(detector.detect_vision("minimax-vl-01"));
        assert!(!detector.detect_vision("gpt-4-turbo"));
    }

    #[test]
    fn test_detect_reasoning() {
        let detector = CapabilityDetector::new();

        assert!(detector.detect_reasoning("o1-preview"));
        assert!(detector.detect_reasoning("o3-mini"));
        assert!(detector.detect_reasoning("deepseek-r1"));
        assert!(detector.detect_reasoning("qwq-32b-preview"));
        assert!(detector.detect_reasoning("glm-z1"));
        assert!(!detector.detect_reasoning("gpt-4o"));
    }

    #[test]
    fn test_model_supports() {
        assert!(model_supports("gpt-4o", "streaming"));
        assert!(model_supports("gpt-4o", "vision"));
        assert!(model_supports("gpt-4o", "function_calling"));
        assert!(model_supports("gpt-4o", "json"));

        assert!(!model_supports("gpt-4-turbo", "vision"));
        assert!(!model_supports("gpt-3.5-turbo", "video"));
    }

    #[test]
    fn test_get_max_context() {
        assert_eq!(get_max_context("gpt-4o"), 128000);
        assert_eq!(get_max_context("claude-3-5-sonnet"), 200000);
        assert_eq!(get_max_context("gemini-1.5-pro"), 1000000);
        assert_eq!(get_max_context("gemini-2.0-flash"), 1000000);
        assert_eq!(get_max_context("qwen-long"), 1000000);
        assert_eq!(get_max_context("deepseek-v3"), 128000);
        assert_eq!(get_max_context("glm-5"), 128000);
    }

    #[test]
    fn test_detector_with_builtin_models() {
        let mut detector = CapabilityDetector::new();

        // Test a built-in model
        let result = detector.detect("gpt-4o-mini").unwrap();
        assert_eq!(result.model, "gpt-4o-mini");
        assert!(result.from_registry);
        assert!(result.capabilities.streaming);
        assert!(result.capabilities.vision);

        // Test a non-built-in model (heuristic)
        let result = detector.detect("custom-model-7b").unwrap();
        assert_eq!(result.model, "custom-model-7b");
        assert!(!result.from_registry);
    }
}
