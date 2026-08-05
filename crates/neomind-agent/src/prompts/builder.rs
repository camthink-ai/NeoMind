//! Prompt generation utilities for the NeoMind AI Agent.
//!
//! ## Architecture
//!
//! Three-layer documentation:
//! 1. System prompt (~2,000 tokens) — core decision rules, always loaded
//! 2. CLI `--help` — command details, loaded on demand by LLM running `neomind <cmd> --help`
//! 3. Skill tool — complex workflows and error troubleshooting, loaded on demand

/// Placeholder for current UTC time in prompts.
pub const CURRENT_TIME_PLACEHOLDER: &str = "{{CURRENT_TIME}}";

/// Placeholder for current local time in prompts.
pub const LOCAL_TIME_PLACEHOLDER: &str = "{{LOCAL_TIME}}";

/// Placeholder for system timezone in prompts.
pub const TIMEZONE_PLACEHOLDER: &str = "{{TIMEZONE}}";

/// Single-file system prompt template. Conditional sections (Vision, Thinking)
/// are wrapped in HTML-comment sentinels, stripped at build time based on flags.
const SYSTEM_PROMPT_TEMPLATE: &str = include_str!("system_prompt.md");

/// Slim system prompt template (~48% smaller). Selected when
/// `NEOMIND_SLIM_PROMPT=1` — an opt-in for small local models that struggle
/// with the full template's token volume. Keeps every hard constraint
/// (language policy, tool-first, no fabrication, batch rule, error recovery)
/// and drops soft style guidance (chitchat examples, narrate-intent, etc.).
/// Must preserve the KV-CACHE BOUNDARY block and the same sentinel markers.
const SYSTEM_PROMPT_SLIM_TEMPLATE: &str = include_str!("system_prompt_slim.md");

const VISION_BEGIN: &str = "<!-- BEGIN_VISION -->";
const VISION_END: &str = "<!-- END_VISION -->";
const THINKING_BEGIN: &str = "<!-- BEGIN_THINKING -->";
const THINKING_END: &str = "<!-- END_THINKING -->";

/// Enhanced prompt builder.
#[derive(Debug, Clone)]
pub struct PromptBuilder {
    /// Whether to include thinking mode instructions
    include_thinking: bool,
    /// Whether this model supports vision/multimodal input
    supports_vision: bool,
}

impl PromptBuilder {
    /// Create a new prompt builder.
    /// The prompt instructs the LLM to respond in the same language as the user's input.
    pub fn new() -> Self {
        Self {
            include_thinking: true,
            supports_vision: false,
        }
    }

    /// Enable or disable thinking mode instructions.
    pub fn with_thinking(mut self, include: bool) -> Self {
        self.include_thinking = include;
        self
    }

    /// Enable or disable vision/multimodal capability.
    /// When enabled, adds instructions for processing images.
    pub fn with_vision(mut self, supports_vision: bool) -> Self {
        self.supports_vision = supports_vision;
        self
    }

    /// Build the enhanced system prompt.
    pub fn build_system_prompt(&self) -> String {
        let template = if std::env::var("NEOMIND_SLIM_PROMPT").as_deref() == Ok("1") {
            SYSTEM_PROMPT_SLIM_TEMPLATE
        } else {
            SYSTEM_PROMPT_TEMPLATE
        };
        let mut prompt = template.to_string();
        if self.supports_vision {
            // Keep content, strip only the sentinel markers.
            prompt = strip_sentinels(&prompt, VISION_BEGIN, VISION_END);
        } else {
            prompt = strip_block(&prompt, VISION_BEGIN, VISION_END);
        }
        if self.include_thinking {
            prompt = strip_sentinels(&prompt, THINKING_BEGIN, THINKING_END);
        } else {
            prompt = strip_block(&prompt, THINKING_BEGIN, THINKING_END);
        }
        prompt
    }

    /// Get intent-specific system prompt addon.
    pub fn get_intent_prompt_addon(&self, intent: &str) -> String {
        match intent {
            "device" => "\n\n## Current Task: Device Management\nFocus on device queries and control operations.".to_string(),
            "data" => "\n\n## Current Task: Data Analysis\nGather real data via tools, then provide insights, root-cause analysis, and actionable recommendations.\n\nUse `neomind device history <id> --metric <name>` for time-series data — never fabricate values.".to_string(),
            "rule" => "\n\n## Current Task: Rule Management\nFocus on creating and modifying automation rules.".to_string(),
            "alert" | "message" => "\n\n## Current Task: Message Management\nFocus on message queries, sending, and status updates.".to_string(),
            "system" => "\n\n## Current Task: System Status\nFocus on system health checks and status queries.".to_string(),
            "help" => "\n\n## Current Task: Help & Documentation\nProvide clear usage instructions and feature overview without calling tools.".to_string(),
            _ => String::new(),
        }
    }
}

impl Default for PromptBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// Remove a conditional block (markers + content) from the prompt.
/// Also consumes one adjacent newline on each side to avoid blank-line accumulation.
fn strip_block(s: &str, begin: &str, end: &str) -> String {
    match (s.find(begin), s.find(end)) {
        (Some(b), Some(e)) if e >= b => {
            let mut start = b;
            // Consume one preceding newline so we don't leave a dangling blank line.
            if start > 0 && s.as_bytes()[start - 1] == b'\n' {
                start -= 1;
            }
            let mut finish = e + end.len();
            // Consume one trailing newline after the end marker.
            if finish < s.len() && s.as_bytes()[finish] == b'\n' {
                finish += 1;
            }
            let mut out = String::with_capacity(s.len());
            out.push_str(&s[..start]);
            out.push_str(&s[finish..]);
            out
        }
        _ => s.to_string(),
    }
}

/// Strip only the sentinel marker lines, keeping the enclosed content.
fn strip_sentinels(s: &str, begin: &str, end: &str) -> String {
    s.replace(&format!("{}\n", begin), "")
        .replace(&format!("\n{}", end), "")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_prompt_builder_default() {
        let builder = PromptBuilder::new();
        let prompt = builder.build_system_prompt();
        assert!(prompt.contains("NeoMind"));
        assert!(prompt.contains("IoT"));
        assert!(prompt.contains("Principles"));
        assert!(!prompt.contains("Visual Understanding"));
    }

    #[test]
    fn test_prompt_without_examples() {
        let builder = PromptBuilder::new();
        let prompt = builder.build_system_prompt();
        assert!(prompt.contains("Principles"));
        assert!(!prompt.contains("Example Dialogs"));
    }

    #[test]
    fn test_prompt_without_thinking() {
        let builder = PromptBuilder::new().with_thinking(false);
        let prompt = builder.build_system_prompt();
        assert!(prompt.contains("Principles"));
        assert!(!prompt.contains("Thinking Mode"));
    }

    #[test]
    fn test_intent_addon() {
        let builder = PromptBuilder::new();
        let addon = builder.get_intent_prompt_addon("data");
        assert!(addon.contains("Data Analysis"));
    }

    #[test]
    fn test_language_policy_in_prompt() {
        let builder = PromptBuilder::new();
        let prompt = builder.build_system_prompt();
        assert!(prompt.contains("Language Policy"));
        assert!(prompt.contains("Highest Priority"));
        let prompt_lower = prompt.to_lowercase();
        assert!(prompt_lower.contains("same language"));
    }

    #[test]
    fn test_no_cli_reference_table() {
        let builder = PromptBuilder::new();
        let prompt = builder.build_system_prompt();
        // CLI reference table should be removed
        assert!(!prompt.contains("CLI Command Reference"));
        // But --help guidance should be present
        assert!(prompt.contains("--help"));
    }

    #[test]
    fn test_no_example_responses() {
        let builder = PromptBuilder::new();
        let prompt = builder.build_system_prompt();
        assert!(!prompt.contains("Example Dialogs"));
    }

    #[test]
    fn test_no_vision_when_disabled() {
        let builder = PromptBuilder::new().with_vision(false);
        let prompt = builder.build_system_prompt();
        assert!(!prompt.contains("Vision"));
    }

    #[test]
    fn test_vision_when_enabled() {
        let builder = PromptBuilder::new().with_vision(true);
        let prompt = builder.build_system_prompt();
        assert!(prompt.contains("Vision"));
        assert!(prompt.contains("analyze images"));
    }

    #[test]
    fn test_key_rules_preserved() {
        let builder = PromptBuilder::new();
        let prompt = builder.build_system_prompt();
        // Tier 1 rules must remain
        assert!(prompt.contains("No Hallucinated Operations"));
        assert!(prompt.contains("Task Workflow"));
        assert!(prompt.contains("BATCH RULE"));
        assert!(prompt.contains("Domain Boundaries"));
    }

    #[test]
    fn test_conditional_blocks_stripped_when_disabled() {
        let with_all = PromptBuilder::new()
            .with_vision(true)
            .with_thinking(true)
            .build_system_prompt();
        let no_vision = PromptBuilder::new()
            .with_vision(false)
            .build_system_prompt();
        let no_thinking = PromptBuilder::new()
            .with_thinking(false)
            .build_system_prompt();

        assert!(with_all.contains("## Vision"));
        assert!(!no_vision.contains("## Vision"));
        assert!(!no_vision.contains("BEGIN_VISION"));

        assert!(with_all.contains("## Thinking Mode"));
        assert!(!no_thinking.contains("## Thinking Mode"));
        assert!(!no_thinking.contains("BEGIN_THINKING"));

        // Default builder has vision disabled, thinking enabled.
        let default_prompt = PromptBuilder::new().build_system_prompt();
        assert!(!default_prompt.contains("## Vision"));
        assert!(default_prompt.contains("## Thinking Mode"));
    }

    #[test]
    fn test_slim_prompt_is_compact_and_preserves_rules() {
        // NEOMIND_SLIM_PROMPT=1 must yield a smaller prompt that still keeps
        // every hard constraint (language policy, tool-first, batch rule) —
        // only soft style guidance is dropped.
        unsafe {
            std::env::set_var("NEOMIND_SLIM_PROMPT", "1");
        }
        let slim = PromptBuilder::new().build_system_prompt();
        unsafe {
            std::env::remove_var("NEOMIND_SLIM_PROMPT");
        }
        let full = PromptBuilder::new().build_system_prompt();

        assert!(slim.len() < full.len(), "slim must be smaller");
        assert!(slim.contains("Language Policy"));
        assert!(slim.contains("No Hallucinated Operations"));
        assert!(slim.contains("BATCH RULE"));
        assert!(slim.contains("Tool-First"));
        assert!(slim.contains("{{CURRENT_TIME}}")); // KV-cache boundary preserved
        assert!(slim.contains("## Environment"));
    }
}
