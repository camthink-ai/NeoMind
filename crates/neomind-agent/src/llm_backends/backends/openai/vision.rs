//! `vision` — split from the former openai.rs monolith.

use super::*;

/// Check if a model supports vision (image input) based on provider and model name.
/// This uses name-based heuristic detection for common vision-capable models.
pub(crate) fn is_vision_model(_provider: &CloudProvider, model_name: &str) -> bool {
    // Primary: centralized layered detection (LiteLLM registry → conservative
    // heuristic). This is authoritative when the registry has an entry.
    if neomind_core::llm::detect_vision_capability(model_name) {
        return true;
    }

    // Fallback: well-known vision families the LiteLLM registry misses under
    // bare aliases (e.g. `claude-3-sonnet`, `gemini-1.5-flash`, `o1`).
    //
    // This MUST stay narrow. The previous version matched bare Qwen
    // text-only commercial tiers (`qwen-max`, `qwen-plus`, `qwen-turbo`,
    // `qwen3-*`, `qwen-3-*`) as vision-capable. Cloud backends built via the
    // instance manager do not receive a `capabilities_override`, so they fell
    // back to this function, reported `supports_multimodal == true` for text
    // models, the chat gating let `image_url` content parts through, and the
    // upstream API rejected the request with
    // `unknown variant image_url, expected text`.
    //
    // The Qwen text tiers are deliberately excluded below — only explicit
    // `-vl`/`vision` variants and the native-multimodal qwen3.5/3.6/3.7
    // series match.
    known_vision_family(model_name)
}

/// Narrow fallback of unambiguous vision-family name patterns. Used only when
/// the layered registry/heuristic detection returns false, to cover cloud
/// models whose bare aliases are absent from the LiteLLM registry.
pub(crate) fn known_vision_family(model_name: &str) -> bool {
    let m = model_name.to_lowercase();
    // Explicit vision markers (suffixes / branding) — unambiguous.
    if m.contains("-vl")
        || m.contains(":vl")
        || m.contains("_vl")
        || m.contains("vision")
        || m.contains("multimodal")
        || m.contains("glm-4v")
        || m.contains("glm-5v")
    {
        return true;
    }
    // OpenAI vision families. o1-preview and o1-mini are text-only.
    if m.contains("gpt-4o")
        || m.contains("gpt-4-turbo")
        || m.contains("gpt-4.1")
        || m.contains("gpt-4-vision")
        || (m.starts_with("gpt-4") && m.contains("vision"))
        || (m.starts_with("o1") && !m.contains("o1-preview") && !m.contains("o1-mini"))
    {
        return true;
    }
    // Anthropic Claude 3+ and Google Gemini are universally multimodal.
    if m.contains("claude-3") || m.contains("claude-4") || m.contains("gemini") {
        return true;
    }
    // Qwen native-multimodal early-fusion series. The bare text tiers
    // (`qwen-max`/`qwen-plus`/`qwen-turbo`, `qwen3-*`, `qwen-3-*`) are
    // intentionally NOT matched here.
    if m.starts_with("qwen3.5") || m.starts_with("qwen3.6") || m.starts_with("qwen3.7") {
        return true;
    }
    false
}
