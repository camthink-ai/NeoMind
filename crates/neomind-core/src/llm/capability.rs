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

        // Try built-in registry first. The hand-curated table (models.rs) is
        // small and can go stale / disagree with the LiteLLM registry (e.g. an
        // old `gpt-4.5` entry marked reasoning:false while the registry says
        // supports_reasoning:true). So after a table hit we OVERRIDE the
        // authoritative fields with the registry's values, which are
        // community-maintained and refreshed. Fields the registry doesn't
        // cover (provider, json_mode, audio, …) keep the table's value.
        if let Some(info) = get_model_info(model) {
            let mut caps = info.capabilities.clone();
            let mut from_registry = false;
            // Reasoning: registry wins over the table.
            if let Some(r) = crate::llm::registry::lookup_reasoning(model) {
                caps.reasoning = r;
                from_registry = true;
            }
            // Vision: registry wins over the table.
            if let Some(v) = crate::llm::registry::lookup_vision(model) {
                caps.vision = v;
                from_registry = true;
            }
            // Max context: registry's max_input_tokens wins when present.
            if let Some(ctx) = crate::llm::registry::lookup_max_input_tokens(model) {
                caps.max_context = Some(ctx);
                from_registry = true;
            }
            let result = CapabilityDetectionResult {
                model: model.to_string(),
                provider: info.provider,
                capabilities: caps,
                from_registry,
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
    ///
    /// Implements a 3-tier layered detection:
    ///
    /// **Tier 1 — LiteLLM registry (authoritative for cloud/commercial models):**
    /// Looks up the embedded `model_registry.json` (2748+ entries sourced from
    /// LiteLLM's community-maintained catalog). Returns immediately if found,
    /// since this data is curated and frequently updated.
    ///
    /// **Tier 2 — Conservative heuristic (for local/Ollama models only):**
    /// Only matches *unambiguous* vision-name patterns. Family-name matches
    /// (e.g. `qwen3.5`, `gemma3`, `mistral3`) are deliberately **NOT** used
    /// because they cause false positives — most of these are text-only, with
    /// only specific `-vl`/`-vision` variants supporting multimodal input.
    ///
    /// **Tier 3 — Default `false`:** Unknown models are assumed text-only.
    /// Callers needing authoritative detection should query the runtime API
    /// (Ollama `/api/show`, llama.cpp `/props`) — see `runtime_capabilities.rs`.
    /// Users can also override via `multimodal_user_override` in storage.
    pub fn detect_vision(&self, model: &str) -> bool {
        // Tier 1: registry lookup
        if let Some(v) = crate::llm::registry::lookup_vision(model) {
            return v;
        }

        // Tier 2: conservative heuristic for models not in registry
        // (local/Ollama, regional providers). See `heuristic_vision_match`
        // for rationale on which patterns are considered unambiguous.
        if crate::llm::registry::heuristic_vision_match(model) {
            return true;
        }

        // Tier 3: unknown — assume text-only (conservative default).
        // False negative is recoverable (user can override); false positive
        // causes silent image drops or hallucinated image analysis.
        false
    }

    /// Detect audio capability.
    ///
    /// Note: `gpt-4o-audio` is matched specifically rather than the bare
    /// `gpt-4o` substring — `gpt-4o-mini` and most `gpt-4o-*` text/vision
    /// variants do NOT accept audio input, and a too-broad match would
    /// cause the agent pipeline to emit audio content parts that the API
    /// rejects. Same class of bug as the historical `is_vision_model`
    /// over-broad heuristic (see MEMORY.md).
    fn detect_audio(&self, model: &str) -> bool {
        model.contains("audio")
            || model.contains("tts")
            || model.contains("asr")
            || model.contains("whisper")
            || model.contains("gpt-4o-audio")
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
    /// Delegates to the shared [`detect_thinking`] so all callers agree.
    fn detect_reasoning(&self, model: &str) -> bool {
        detect_thinking(model)
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
        .unwrap_or(128000)
}

/// Detect vision/multimodal capability from model name.
/// This is a standalone function that can be used without creating a CapabilityDetector.
pub fn detect_vision_capability(model: &str) -> bool {
    let detector = CapabilityDetector::new();
    detector.detect_vision(model)
}

/// Detect whether a model supports extended thinking/reasoning (Qwen3,
/// DeepSeek-R1, GPT-OSS, o1/o3, QwQ, GLM-Z1).
///
/// Single source of truth for "thinking model" decisions across the codebase.
/// Historically four sites implemented this independently with divergent
/// rules (`qwen3-vl` was thinking on one path and not another; `qwen2.5`
/// was wrongly flagged; `qwq`/`glm-z1`/`gpt-oss` were missed). All callers
/// should use this function.
///
/// Note: modern multimodal models (qwen3-vl, gemini-flash-thinking, etc.)
/// support both vision and thinking, so `-vl` is NOT excluded here.
pub fn detect_thinking(model: &str) -> bool {
    // Authoritative source first: the LiteLLM registry's `supports_reasoning`
    // field (620+ models marked true, incl. qwen3.5-plus / gpt-5 / deepseek-v4).
    // A definitive Some(false) wins over the name heuristic below — the
    // registry is curated and knows a model is non-reasoning even when its
    // name looks reasoning-ish. None (not in registry) falls through.
    if let Some(reg) = crate::llm::registry::lookup_reasoning(model) {
        return reg;
    }

    let name_lower = model.to_lowercase();

    // Qwen3 family (qwen3, qwen3:2b, qwen3-vl, qwen3.5-plus, …)
    if name_lower.starts_with("qwen3") || name_lower.contains("qwen3-") {
        return true;
    }
    // GPT-OSS (OpenAI's reasoning model)
    if name_lower.contains("gpt-oss") {
        return true;
    }
    // DeepSeek reasoning models (deepseek-r1, deepseek-r1-distill-*, deepseek v3.1)
    if name_lower.contains("deepseek-r1")
        || name_lower.contains("deepseek-r")
        || name_lower.contains("deepseek v3.1")
        || name_lower.contains("deepseek-v3.1")
    {
        return true;
    }
    // Reasoning families
    if name_lower.contains("qwq")
        || name_lower.contains("glm-z1")
        || name_lower.contains("thinking")
        || name_lower.contains("reasoning")
    {
        return true;
    }
    // o1 / o3 family (use word-ish matching to avoid hitting "o10", "ro1", etc.)
    if name_lower.contains("o1-preview")
        || name_lower.contains("o1-mini")
        || name_lower.contains("o1-pro")
        || name_lower.contains("o3-mini")
        || name_lower.contains("o3-pro")
    {
        return true;
    }
    false
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

        // 支持视觉的模型
        assert!(detector.detect_vision("gpt-4o"));
        assert!(detector.detect_vision("gpt-4o-mini"));
        assert!(detector.detect_vision("gpt-4-turbo"));
        assert!(detector.detect_vision("qwen-vl-max"));
        assert!(detector.detect_vision("qwen2.5-vl-7b-instruct"));
        assert!(detector.detect_vision("qwen3-vl-plus"));
        assert!(detector.detect_vision("claude-3-5-sonnet"));
        assert!(detector.detect_vision("claude-opus-4"));
        assert!(detector.detect_vision("gemini-2.0-flash"));
        assert!(detector.detect_vision("minimax-vl-01"));
        assert!(detector.detect_vision("glm-4v-plus"));
        assert!(detector.detect_vision("grok-2-vision"));

        // 不支持视觉的模型
        assert!(!detector.detect_vision("gpt-3.5-turbo"));
        assert!(!detector.detect_vision("gpt-4")); // 不带 turbo/vision 的基础版
        assert!(!detector.detect_vision("o1-preview"));
        assert!(!detector.detect_vision("o3-mini"));
        assert!(!detector.detect_vision("qwen-turbo"));
        assert!(!detector.detect_vision("qwen-coder-plus"));
        assert!(!detector.detect_vision("deepseek-chat"));
        assert!(!detector.detect_vision("deepseek-r1"));
        assert!(!detector.detect_vision("glm-4-plus"));
        assert!(!detector.detect_vision("grok-3"));
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
    fn test_detect_thinking() {
        // Thinking models
        assert!(detect_thinking("qwen3:32b"));
        assert!(detect_thinking("qwen3-vl:2b"), "multimodal + thinking");
        assert!(detect_thinking("qwen3.5-plus"));
        assert!(detect_thinking("deepseek-r1"));
        assert!(detect_thinking("deepseek-r1-distill-llama-8b"));
        assert!(detect_thinking("gpt-oss-20b"));
        assert!(detect_thinking("qwq-32b-preview"));
        assert!(detect_thinking("glm-z1"));
        assert!(detect_thinking("o1-preview"));
        assert!(detect_thinking("o3-mini"));

        // Non-thinking models
        assert!(
            !detect_thinking("qwen2.5:0.5b"),
            "qwen2.5 is not a thinking model"
        );
        assert!(!detect_thinking("qwen2:7b"));
        assert!(!detect_thinking("llama3.1:8b"));
        assert!(!detect_thinking("gemma3:4b"));
        assert!(!detect_thinking("gpt-4o"));
        assert!(!detect_thinking("mistral"));
    }

    #[test]
    fn test_detect_thinking_uses_registry() {
        // The registry is the authoritative source. qwen3.5-plus is marked
        // supports_reasoning=true there, so detect_thinking must agree even
        // though it also matches the name heuristic.
        assert!(detect_thinking("dashscope/qwen3.5-plus"));
        assert!(
            detect_thinking("qwen3.5-plus"),
            "bare alias falls back to name heuristic"
        );

        // gpt-4o has no supports_reasoning field → falls through to the name
        // heuristic, which correctly says non-thinking.
        assert!(!detect_thinking("gpt-4o"));
    }

    #[test]
    fn test_lookup_reasoning() {
        use crate::llm::registry::lookup_reasoning;
        // Provider-prefixed key resolves; a model without the field returns
        // None (unknown), and an unknown model returns None too.
        assert_eq!(lookup_reasoning("dashscope/qwen3.5-plus"), Some(true));
        // gpt-4o has no supports_reasoning field in the current registry → None.
        assert_eq!(lookup_reasoning("gpt-4o"), None);
        assert_eq!(lookup_reasoning("definitely-not-a-real-model-xyz"), None);
    }

    #[test]
    fn test_detect_registry_overrides_manual_table() {
        // gpt-5 is in BOTH the manual table (models.rs, max_context=1_000_000)
        // and the LiteLLM registry (max_input_tokens=272000). detect() must
        // let the registry's authoritative max_context win over the stale
        // manual value.
        let mut detector = CapabilityDetector::new();
        let result = detector
            .detect("gpt-5")
            .expect("gpt-5 is in the manual table");
        assert_eq!(
            result.capabilities.max_context,
            Some(272000),
            "registry max_input_tokens overrides manual table max_context"
        );
        assert!(result.from_registry, "overridden fields flag from_registry");
    }

    #[test]
    fn test_model_supports() {
        assert!(model_supports("gpt-4o", "streaming"));
        assert!(model_supports("gpt-4o", "vision"));
        assert!(model_supports("gpt-4o", "function_calling"));
        assert!(model_supports("gpt-4o", "json"));

        // gpt-4-turbo 支持视觉
        assert!(model_supports("gpt-4-turbo", "vision"));

        // 不支持视觉的模型
        assert!(!model_supports("gpt-3.5-turbo", "vision"));
        assert!(!model_supports("o1-preview", "vision"));
        assert!(!model_supports("qwen-turbo", "vision"));
    }

    #[test]
    fn test_detect_audio_gpt4o_no_false_positive() {
        // Regression: bare `gpt-4o` substring used to match every gpt-4o*
        // variant including gpt-4o-mini (text/vision only). This caused the
        // pipeline to send audio content parts to text-only backends. Only
        // the explicitly audio-named variants should match.
        assert!(model_supports("gpt-4o-audio", "audio"));
        assert!(model_supports("gpt-4o-audio-preview", "audio"));
        assert!(model_supports("gpt-4o-audio-2024-10-01", "audio"));

        // Critical: these MUST NOT match.
        assert!(!model_supports("gpt-4o", "audio"));
        assert!(!model_supports("gpt-4o-mini", "audio"));
        assert!(!model_supports("gpt-4o-2024-08-06", "audio"));
        assert!(!model_supports("gpt-4o-2024-11-20", "audio"));

        // Positive cases for other audio families still work.
        assert!(model_supports("qwen-omni-turbo", "audio"));
        assert!(model_supports("qwen2-audio-7b", "audio"));
        assert!(model_supports("whisper-large-v3", "audio"));
        assert!(model_supports("tts-1", "audio"));

        // Sanity: non-audio models stay negative.
        assert!(!model_supports("gpt-4-turbo", "audio"));
        assert!(!model_supports("claude-3-5-sonnet", "audio"));
        assert!(!model_supports("qwen-max", "audio"));
        assert!(!model_supports("deepseek-chat", "audio"));
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
        let result = detector.detect("gpt-4o").unwrap();
        assert_eq!(result.model, "gpt-4o");
        assert!(result.from_registry);
        assert!(result.capabilities.streaming);
        assert!(result.capabilities.vision);

        // Test a non-built-in model (heuristic)
        let result = detector.detect("custom-model-7b").unwrap();
        assert_eq!(result.model, "custom-model-7b");
        assert!(!result.from_registry);
    }
}
