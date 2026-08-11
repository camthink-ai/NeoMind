//! Model capability detection module.
//!
//! Name-based capability detection backed by the LiteLLM registry (vision,
//! reasoning, max_context, function_calling) plus conservative name heuristics
//! for local/Ollama models absent from the registry. The old hand-curated
//! manual table (`models.rs`) and `CapabilityDetector` have been removed —
//! the registry is the single authoritative source for the fields it covers;
//! audio and the tools fallback use name heuristics (no curated data source
//! exists for those).

use crate::llm::registry;

/// Detect vision/multimodal capability from model name.
///
/// 3-tier layered detection:
///
/// **Tier 1 — LiteLLM registry (authoritative for cloud/commercial models):**
/// Looks up the embedded `model_registry.json` (`supports_vision`). Returns
/// immediately if found — this data is curated and refreshed with each release.
///
/// **Tier 2 — Conservative heuristic (for local/Ollama models only):**
/// Only matches *unambiguous* vision-name patterns via `heuristic_vision_match`.
/// Family-name matches (`qwen3`, `gemma3`, `mistral3`) are deliberately **NOT**
/// used — most are text-only, with only specific `-vl`/`-vision` variants
/// supporting multimodal input.
///
/// **Tier 3 — Default `false`:** unknown models are assumed text-only. False
/// negative is recoverable (user override, or runtime API: Ollama `/api/show`,
/// llama.cpp `/props`); false positive causes silent image drops or
/// hallucinated image analysis.
pub fn detect_vision_capability(model: &str) -> bool {
    if let Some(v) = registry::lookup_vision(model) {
        return v;
    }
    registry::heuristic_vision_match(model)
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
    if let Some(reg) = registry::lookup_reasoning(model) {
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

/// Detect audio capability from model name.
///
/// Only explicitly audio-named variants match. Bare `gpt-4o` does NOT match
/// (`gpt-4o-mini` and most `gpt-4o-*` are text/vision only) — too-broad
/// matching causes the pipeline to emit audio content parts the API rejects.
/// Same class of bug as the historical over-broad vision heuristic.
pub fn supports_audio(model: &str) -> bool {
    let n = model.to_lowercase();
    n.contains("audio")
        || n.contains("tts")
        || n.contains("asr")
        || n.contains("whisper")
        || n.contains("gpt-4o-audio")
        || n.contains("qwen-audio")
        || n.contains("qwen-tts")
        || n.contains("qwen-omni")
        || n.contains("minimax-speech")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_vision() {
        // 支持视觉的模型
        assert!(detect_vision_capability("gpt-4o"));
        assert!(detect_vision_capability("gpt-4o-mini"));
        assert!(detect_vision_capability("gpt-4-turbo"));
        assert!(detect_vision_capability("qwen-vl-max"));
        assert!(detect_vision_capability("qwen2.5-vl-7b-instruct"));
        assert!(detect_vision_capability("qwen3-vl-plus"));
        assert!(detect_vision_capability("claude-3-5-sonnet"));
        assert!(detect_vision_capability("claude-opus-4"));
        assert!(detect_vision_capability("gemini-2.0-flash"));
        assert!(detect_vision_capability("minimax-vl-01"));
        assert!(detect_vision_capability("glm-4v-plus"));
        assert!(detect_vision_capability("grok-2-vision"));

        // 不支持视觉的模型
        assert!(!detect_vision_capability("gpt-3.5-turbo"));
        assert!(!detect_vision_capability("gpt-4")); // 不带 turbo/vision 的基础版
        assert!(!detect_vision_capability("o1-preview"));
        assert!(!detect_vision_capability("o3-mini"));
        assert!(!detect_vision_capability("qwen-turbo"));
        assert!(!detect_vision_capability("qwen-coder-plus"));
        assert!(!detect_vision_capability("deepseek-chat"));
        assert!(!detect_vision_capability("deepseek-r1"));
        assert!(!detect_vision_capability("glm-4-plus"));
        assert!(!detect_vision_capability("grok-3"));
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
    fn test_supports_audio_no_false_positive() {
        // Regression: bare `gpt-4o` substring used to match every gpt-4o*
        // variant including gpt-4o-mini (text/vision only). This caused the
        // pipeline to send audio content parts to text-only backends. Only
        // the explicitly audio-named variants should match.
        assert!(supports_audio("gpt-4o-audio"));
        assert!(supports_audio("gpt-4o-audio-preview"));
        assert!(supports_audio("gpt-4o-audio-2024-10-01"));

        // Critical: these MUST NOT match.
        assert!(!supports_audio("gpt-4o"));
        assert!(!supports_audio("gpt-4o-mini"));
        assert!(!supports_audio("gpt-4o-2024-08-06"));
        assert!(!supports_audio("gpt-4o-2024-11-20"));

        // Positive cases for other audio families still work.
        assert!(supports_audio("qwen-omni-turbo"));
        assert!(supports_audio("qwen2-audio-7b"));
        assert!(supports_audio("whisper-large-v3"));
        assert!(supports_audio("tts-1"));

        // Sanity: non-audio models stay negative.
        assert!(!supports_audio("gpt-4-turbo"));
        assert!(!supports_audio("claude-3-5-sonnet"));
        assert!(!supports_audio("qwen-max"));
        assert!(!supports_audio("deepseek-chat"));
    }
}
