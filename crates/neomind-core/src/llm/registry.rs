//! Curated model capability registry, embedded at compile time.
//!
//! The data file `data/model_registry.json` is sourced from LiteLLM's
//! community-maintained `model_prices_and_context_window.json` (2988 entries
//! as of 2026-08-11, 778 marked `supports_reasoning: true`).
//!
//! This module implements the same 3-tier fallback chain LiteLLM uses:
//!
//! 1. Provider-prefixed key: `"deepseek/deepseek-chat"`, `"openai/gpt-4o"`
//! 2. Bare model key: `"deepseek-chat"`, `"gpt-4o"`
//! 3. (no further fallback — return `None` = unknown)
//!
//! `Option<bool>` distinguishes three states:
//! - `Some(true)`  — known to support vision
//! - `Some(false)` — known NOT to support vision
//! - `None`        — not in registry, caller should fall back to heuristic
//!
//! Update the registry by re-downloading from
//! <https://github.com/BerriAI/litellm/blob/main/model_prices_and_context_window.json>
//! into `crates/neomind-core/src/assets/model_registry.json`.

use std::collections::HashMap;
use std::sync::OnceLock;

/// Embedded registry JSON (compile-time).
const REGISTRY_JSON: &str = include_str!("../assets/model_registry.json");

/// Parsed registry entry. We parse the whole JSON as `serde_json::Value`
/// first and store it raw — that way one malformed entry (e.g. the
/// `sample_spec` schema doc with strings where numbers are expected)
/// doesn't break the entire registry. Fields are extracted lazily at
/// lookup time with type-tolerant helpers below.
#[derive(Debug, Clone)]
struct RegistryEntry {
    raw: serde_json::Value,
}

impl RegistryEntry {
    fn bool_field(&self, key: &str) -> Option<bool> {
        self.raw.get(key).and_then(|v| v.as_bool())
    }

    fn usize_field(&self, key: &str) -> Option<usize> {
        self.raw
            .get(key)
            .and_then(|v| v.as_u64())
            .map(|n| n as usize)
    }
}

/// Parsed registry map: model_key -> entry.
static REGISTRY: OnceLock<HashMap<String, RegistryEntry>> = OnceLock::new();

fn registry() -> &'static HashMap<String, RegistryEntry> {
    REGISTRY.get_or_init(|| {
        // Parse as raw JSON first to tolerate per-entry type mismatches.
        let parsed: HashMap<String, serde_json::Value> = serde_json::from_str(REGISTRY_JSON)
            .unwrap_or_else(|e| {
                tracing::error!(
                    error = %e,
                    "Failed to parse embedded model_registry.json — falling back to empty registry"
                );
                HashMap::new()
            });
        let entries: HashMap<String, RegistryEntry> = parsed
            .into_iter()
            .map(|(k, v)| (k, RegistryEntry { raw: v }))
            .collect();
        let stats = stats_internal(&entries);
        tracing::info!(
            total = stats.total,
            vision_count = stats.vision_count,
            "Loaded model capability registry"
        );
        entries
    })
}

fn stats_internal(map: &HashMap<String, RegistryEntry>) -> RegistryStats {
    let total = map.len();
    let vision_count = map
        .values()
        .filter(|e| e.bool_field("supports_vision") == Some(true))
        .count();
    let non_vision_count = map
        .values()
        .filter(|e| e.bool_field("supports_vision") == Some(false))
        .count();
    RegistryStats {
        total,
        vision_count,
        non_vision_count,
        unknown_count: total - vision_count - non_vision_count,
    }
}

/// Look up whether a model supports vision.
///
/// Returns:
/// - `Some(true)` if the registry marks it `supports_vision: true`
/// - `Some(false)` if the registry marks it `supports_vision: false`
/// - `None` if the model is not in the registry (or has no field set)
///
/// Implements LiteLLM's 3-tier fallback: provider-prefixed key → bare key → None.
pub fn lookup_vision(model: &str) -> Option<bool> {
    lookup_field(model, "supports_vision", |e, k| e.bool_field(k))
}

/// Look up the max input tokens for a model.
pub fn lookup_max_input_tokens(model: &str) -> Option<usize> {
    lookup_field(model, "max_input_tokens", |e, k| e.usize_field(k))
}

/// Look up whether a model supports reasoning/thinking.
///
/// Returns:
/// - `Some(true)` if the registry marks it `supports_reasoning: true`
/// - `Some(false)` if the registry marks it `supports_reasoning: false`
/// - `None` if the model is not in the registry (or has no field set)
///
/// Mirrors `lookup_vision` (same 3-tier + alias resolution). This is the
/// authoritative data source for `detect_thinking` — the name heuristic is
/// only a fallback for models absent from the registry.
pub fn lookup_reasoning(model: &str) -> Option<bool> {
    lookup_field(model, "supports_reasoning", |e, k| e.bool_field(k))
}

/// Look up whether a model supports function/tool calling.
///
/// Returns `Some(bool)` when the registry has a `supports_function_calling`
/// field for the model, else `None`. Mirrors `lookup_vision` / `lookup_reasoning`
/// (same 3-tier + alias resolution). Authoritative source for
/// `detect_tools_capability`.
pub fn lookup_function_calling(model: &str) -> Option<bool> {
    lookup_field(model, "supports_function_calling", |e, k| e.bool_field(k))
}

/// Resolve well-known short aliases to their registry canonical keys.
///
/// Many providers accept short aliases like `claude-3-5-sonnet` or `gpt-4-turbo`
/// that don't appear verbatim in the LiteLLM registry (which stores dated
/// or provider-prefixed forms). This returns one or more candidate canonical
/// keys to try, in priority order.
///
/// **Input must already be lowercased** by the caller (`lookup_field` does
/// this). Comparisons here are exact-match against lowercase patterns.
///
/// This is intentionally a small, hand-curated table — providers rarely
/// introduce new aliases, and adding them here is a one-line change.
fn aliases_for(model: &str) -> Vec<&'static str> {
    // Anthropic family. LiteLLM's bare-key coverage is spotty for the 3.5
    // generation, so we fall back to the vertex_ai entries which are always
    // present and reflect the same model capabilities.
    if model.starts_with("claude-3-5-sonnet") {
        return vec!["vertex_ai/claude-3-5-sonnet"];
    }
    if model.starts_with("claude-3-5-haiku") {
        return vec!["vertex_ai/claude-3-5-haiku"];
    }
    if model.starts_with("claude-3-opus") {
        return vec!["claude-3-opus-20240229"];
    }
    if model.starts_with("claude-3-sonnet") {
        return vec!["claude-3-sonnet-20240229"];
    }
    if model.starts_with("claude-3-haiku") {
        return vec!["claude-3-haiku-20240307"];
    }
    // Claude 4 family — bare aliases like `claude-opus-4`, `claude-sonnet-4`.
    if model.starts_with("claude-opus-4-") || model == "claude-opus-4" {
        return vec!["claude-opus-4-20250514"];
    }
    if model.starts_with("claude-sonnet-4-") || model == "claude-sonnet-4" {
        return vec!["claude-sonnet-4-20250514"];
    }
    if model.starts_with("claude-haiku-4-") || model == "claude-haiku-4" {
        return vec!["claude-haiku-4-5-20251001"];
    }
    // OpenAI aliases — `gpt-4-turbo` and `-latest` variants
    if model == "gpt-4-turbo" || model == "gpt-4-turbo-latest" {
        return vec!["gpt-4-turbo-2024-04-09"];
    }
    Vec::new()
}

/// Heuristic patterns that are unambiguous indicators of vision capability
/// for models NOT in the LiteLLM registry (typically local/Ollama models or
/// regional providers like Zhipu GLM).
///
/// These patterns are intentionally narrow: a false positive (claiming a
/// text-only model supports vision) causes silent image drops or hallucinated
/// analysis; a false negative just means the user needs to manually enable
/// vision. So we err on the side of caution.
pub fn heuristic_vision_match(model: &str) -> bool {
    let m = model.to_lowercase();
    // Generic vision-suffix variants
    if m.contains("-vl")
        || m.contains(":vl")
        || m.contains("_vl")
        || m.ends_with("vision")
        || m.contains("-vision")
        || m.contains("_vision")
    {
        return true;
    }
    // Well-known vision model families (explicit vision branding)
    if m.contains("llava")
        || m.contains("moondream")
        || m.contains("minicpm-v")
        || m.contains("pixtral")
        || m.contains("cogvlm")
        || m.contains("internvl")
        || m.contains("yi-vl")
        || m.contains("qvq")
    // Qwen visual reasoning family
    {
        return true;
    }
    // GLM uses `4v` to mark vision variants (glm-4v, glm-4v-plus, glm-4v-flash).
    // Pattern is specific enough not to collide with other model families.
    if m.contains("glm-4v") || m.contains("glm-5v") {
        return true;
    }
    // Native multimodal families — ALL variants in these series support vision.
    // Qwen 3.5/3.6: native multimodal (early fusion), all sizes support vision.
    // Gemma 3: native multimodal, all sizes support vision.
    if m.starts_with("qwen3.5")
        || m.starts_with("qwen3.6")
        || m.starts_with("qwen3.7")
        || m.starts_with("gemma3")
    {
        return true;
    }
    false
}

/// Generic 3-tier lookup following LiteLLM's pattern, plus alias resolution.
///
/// **Case handling**: the LiteLLM registry stores keys in lowercase. We
/// lowercase the input at entry.
///
/// **Tag stripping**: Ollama models use `model:tag` (e.g. `gpt-4o:latest`)
/// but the registry never contains tagged variants. We try the original form
/// first to preserve keys that DO contain a colon (e.g.
/// `anthropic.claude-3-5-sonnet-20240620-v1:0` for AWS Bedrock), then fall
/// back to the tag-stripped form for Ollama compatibility.
fn lookup_field<T: Copy>(
    model: &str,
    field: &str,
    extractor: fn(&RegistryEntry, &str) -> Option<T>,
) -> Option<T> {
    let reg = registry();
    let model_lower = model.to_lowercase();

    // Tier 1: as-is. Important: do this BEFORE tag stripping so AWS Bedrock
    // keys like `anthropic.claude-3-5-sonnet-20240620-v1:0` match correctly.
    if let Some(entry) = reg.get(model_lower.as_str()) {
        if let Some(v) = extractor(entry, field) {
            return Some(v);
        }
    }

    // Tier 2: if the key has a provider prefix ("openai/gpt-4o"), try the
    // bare name ("gpt-4o"). LiteLLM issue #20885.
    //
    // We also try tag-stripped form of the bare name here so that combined
    // forms like `OpenAI/GPT-4O:Latest` resolve correctly. Without this
    // chaining, Tier 2.6 (tag strip on `model_lower`) would yield
    // `openai/gpt-4o` — not `gpt-4o` — and miss the registry entry.
    if let Some((_provider, bare)) = model_lower.split_once('/') {
        if let Some(entry) = reg.get(bare) {
            if let Some(v) = extractor(entry, field) {
                return Some(v);
            }
        }
        // Tag-strip on the bare name (covers prefix+tag combinations).
        if let Some((bare_base, _tag)) = bare.rsplit_once(':') {
            if !bare_base.is_empty() {
                if let Some(entry) = reg.get(bare_base) {
                    if let Some(v) = extractor(entry, field) {
                        return Some(v);
                    }
                }
            }
        }
        // Aliases on the bare name (covers `openai/gpt-4-turbo` etc.).
        for alias in aliases_for(bare) {
            if let Some(entry) = reg.get(alias) {
                if let Some(v) = extractor(entry, field) {
                    return Some(v);
                }
            }
        }
    }

    // Tier 2.5: alias resolution for common short forms (e.g. claude-3-5-sonnet
    // → vertex_ai/claude-3-5-sonnet). Without this, the dated-key-only registry
    // would miss real-world usage where callers pass the alias.
    for alias in aliases_for(&model_lower) {
        if let Some(entry) = reg.get(alias) {
            if let Some(v) = extractor(entry, field) {
                return Some(v);
            }
        }
    }

    // Tier 2.6: Ollama-style `:tag` stripping. Only attempted after all the
    // above tiers failed, so legitimate colon-containing keys (AWS Bedrock
    // `:N` version suffix, OpenAI `ft:` prefix) are matched correctly first.
    if let Some((base, _tag)) = model_lower.rsplit_once(':') {
        if !base.is_empty() {
            if let Some(entry) = reg.get(base) {
                if let Some(v) = extractor(entry, field) {
                    return Some(v);
                }
            }
        }
    }

    // Tier 3: unknown
    None
}

/// Return registry statistics for diagnostic logging.
pub fn stats() -> RegistryStats {
    stats_internal(registry())
}

#[derive(Debug, Clone)]
pub struct RegistryStats {
    pub total: usize,
    pub vision_count: usize,
    pub non_vision_count: usize,
    pub unknown_count: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_loads_successfully() {
        let s = stats();
        // Sanity check: should be 2000+ entries
        assert!(s.total > 2000, "registry seems empty: {:?}", s);
        assert!(s.vision_count > 500, "vision entries seem low: {:?}", s);
        println!("Registry stats: {:?}", s);
    }

    #[test]
    fn gpt_4o_supports_vision() {
        assert_eq!(lookup_vision("gpt-4o"), Some(true));
    }

    #[test]
    fn claude_3_opus_supports_vision() {
        // LiteLLM registry uses date-versioned keys (e.g. claude-3-opus-20240229).
        // Bare "claude-3-opus" is NOT a registry key — callers must use the dated form
        // OR the cloud detector at capability.rs must add its own per-family logic.
        assert_eq!(lookup_vision("claude-3-opus-20240229"), Some(true));
    }

    #[test]
    fn provider_prefixed_lookup() {
        // LiteLLM's tier-1 form: provider/model
        assert_eq!(lookup_vision("openai/gpt-4o"), Some(true));
    }

    #[test]
    fn bare_fallback_after_prefixed_miss() {
        // Some prefixed keys don't have an entry but the bare key does.
        // Hard to find a stable example in the registry — this is a smoke test.
        let v = lookup_vision("some-made-up-prefix/gpt-4o");
        assert_eq!(v, Some(true));
    }

    #[test]
    fn unknown_model_returns_none() {
        assert_eq!(lookup_vision("zzz-not-a-real-model-xyz"), None);
    }

    #[test]
    fn deepseek_chat_not_vision() {
        // Per LiteLLM registry: deepseek-chat is text-only
        let v = lookup_vision("deepseek-chat");
        // Could be Some(false) or None depending on the registry version
        assert_ne!(v, Some(true), "deepseek-chat should not be vision");
    }

    #[test]
    fn max_input_tokens_for_gpt_4o() {
        let tokens = lookup_max_input_tokens("gpt-4o");
        assert!(tokens.is_some());
        assert!(tokens.unwrap() > 100_000);
    }

    #[test]
    fn alias_claude_3_5_sonnet_resolves() {
        // Bare alias — registry only has dated forms.
        assert_eq!(lookup_vision("claude-3-5-sonnet"), Some(true));
        assert_eq!(lookup_vision("claude-3-5-sonnet-latest"), Some(true));
        assert_eq!(lookup_vision("claude-3-opus"), Some(true));
    }

    #[test]
    fn alias_gpt_4_turbo_resolves() {
        assert_eq!(lookup_vision("gpt-4-turbo"), Some(true));
    }

    #[test]
    fn case_insensitive_lookup() {
        // Mixed-case input should resolve identically to lowercase form.
        assert_eq!(lookup_vision("GPT-4O"), Some(true));
        assert_eq!(lookup_vision("Claude-3-5-Sonnet"), Some(true));
        assert_eq!(lookup_vision("Claude-Opus-4"), Some(true));
        // Provider prefix with uppercase provider/model
        assert_eq!(lookup_vision("OpenAI/GPT-4O"), Some(true));
    }

    #[test]
    fn case_insensitive_unknown_stays_none() {
        assert_eq!(lookup_vision("ZZZ-NOT-A-MODEL"), None);
    }

    #[test]
    fn ollama_tag_suffix_stripped() {
        // Ollama models often have `:tag` suffix; the registry never does.
        // The lookup should strip the tag and find the base name.
        assert_eq!(lookup_vision("gpt-4o:latest"), Some(true));
        assert_eq!(lookup_vision("GPT-4O:Latest"), Some(true)); // case + tag
        assert_eq!(lookup_vision("claude-3-opus:2024"), Some(true));
        // Unknown model with tag still returns None
        assert_eq!(lookup_vision("zzz-fake:latest"), None);
        // max_input_tokens also benefits from tag stripping
        assert!(lookup_max_input_tokens("gpt-4o:latest").is_some());
    }

    #[test]
    fn prefix_tag_combined_resolves() {
        // Combined form: provider prefix + tag suffix (with mixed case).
        // Tier 2 strips prefix → `gpt-4o:latest`, then tag-strip on bare → `gpt-4o`.
        assert_eq!(lookup_vision("OpenAI/GPT-4O:Latest"), Some(true));
        assert_eq!(lookup_vision("openai/gpt-4o:latest"), Some(true));
        // Prefix + alias form
        assert_eq!(lookup_vision("openai/gpt-4-turbo:latest"), Some(true));
    }

    #[test]
    fn aws_bedrock_colon_keys_preserved() {
        // AWS Bedrock model IDs use `:N` as version suffix, e.g.
        // `anthropic.claude-3-5-sonnet-20240620-v1:0`. These must NOT be
        // tag-stripped — Tier 1 must match them directly.
        assert_eq!(
            lookup_vision("anthropic.claude-3-5-sonnet-20240620-v1:0"),
            Some(true)
        );
        assert_eq!(
            lookup_vision("anthropic.claude-3-opus-20240229-v1:0"),
            Some(true)
        );
        // OpenAI fine-tune prefix `ft:` also contains a colon.
        let ft = lookup_vision("ft:gpt-4o-2024-08-06");
        assert_eq!(ft, Some(true));
    }
}
