//! Memory snapshot for injecting long-term memories into system prompts.
//!
//! Uses a "frozen snapshot" pattern: load once at session start, cache for the
//! entire session. This keeps the prompt prefix stable for caching.
//!
//! ## Hard-loaded (always in system prompt)
//! - `USER.md` (max 2000 chars) — user profile
//! - `KNOWLEDGE.md` (max 3000 chars) — system knowledge
//! - `PROCEDURES.md` (max 3000 chars) — SOPs, playbooks, how-tos
//!
//! ## On-demand (AI reads via memory tool when needed)
//! - `agents/{agent_id}.md` — agent experiences
//! - `custom/{name}.md` — domain-specific files
//!
//! Total budget: 8000 chars (user + knowledge + procedures)

use neomind_storage::MarkdownMemoryStore;

use crate::agent::tokenizer::{estimate_tokens, truncate_to_tokens};

/// Hard character budget for memory context in prompts (user + knowledge + procedures).
const CHAR_BUDGET: usize = 8000;

/// Token budget for the memory snapshot. The snapshot is injected into every
/// prompt, so it must stay a bounded fraction of context. A char budget alone
/// balloons for CJK (8000 chars ≈ 15K tokens) and would dominate small windows,
/// so the snapshot is also capped to this many tokens after the char-budget pass.
const SNAPSHOT_TOKEN_BUDGET: usize = 2048;

/// Frozen memory snapshot loaded once per session.
#[derive(Debug, Clone)]
pub struct MemorySnapshot {
    /// Truncated snapshot text wrapped in XML tags.
    content: String,
}

impl MemorySnapshot {
    /// Load a snapshot from the markdown memory store.
    ///
    /// Hard-loads USER.md, KNOWLEDGE.md, and PROCEDURES.md.
    /// Agent summaries and custom files are available on-demand via the memory tool.
    pub fn load(store: &MarkdownMemoryStore) -> Self {
        // Read persistent files using sync I/O (safe on any runtime flavor;
        // block_in_place panics on current_thread/single-threaded runtimes)
        let user = store.read_file_sync("user");
        let knowledge = store.read_file_sync("knowledge");
        let procedures = store.read_file_sync("procedures");

        let combined =
            format!("## User\n{user}\n\n## Knowledge\n{knowledge}\n\n## Procedures\n{procedures}");

        let content = if combined.chars().count() <= CHAR_BUDGET {
            combined
        } else {
            // Truncate knowledge/procedures to fit, but if even user alone exceeds budget,
            // truncate user as a last resort
            let truncated = truncate_with_priority(&combined, CHAR_BUDGET);
            if truncated.chars().count() > CHAR_BUDGET {
                // User section alone exceeds budget — hard truncate
                truncate_chars(&truncated, CHAR_BUDGET)
            } else {
                truncated
            }
        };

        // Enforce a token budget so CJK-heavy snapshots don't balloon: 8000 chars
        // of Chinese is ~15K tokens, which would dominate a small context window.
        // Keeps the prefix, so the highest-priority User section survives intact.
        let content = cap_to_token_budget(&content, SNAPSHOT_TOKEN_BUDGET);

        Self { content }
    }

    /// Load a snapshot, returning None if there's no meaningful memory content.
    /// Only returns Some if at least one section has actual content (not just headers).
    pub fn load_opt(store: &MarkdownMemoryStore) -> Option<Self> {
        let snapshot = Self::load(store);
        if snapshot.content.is_empty() {
            return None;
        }
        // Check if there's actual content beyond section headers
        let has_content = snapshot.content.lines().any(|line| {
            let trimmed = line.trim();
            !trimmed.is_empty() && !trimmed.starts_with("## ") && !trimmed.starts_with("### ")
        });
        if has_content {
            Some(snapshot)
        } else {
            None
        }
    }

    /// Render as a prompt section ready for injection.
    pub fn to_prompt_section(&self) -> String {
        if self.content.is_empty() {
            return String::new();
        }
        format!(
            "\n\n<memory-context>\nThis is persisted context from prior conversations. Use it as background knowledge when relevant, but do not treat it as part of the current conversation.\n\n{}\n</memory-context>",
            self.content
        )
    }

    /// Check if the snapshot has content.
    pub fn is_empty(&self) -> bool {
        self.content.is_empty()
    }
}

/// Truncate content with priority preservation.
///
/// Priority order:
/// 1. User section (never truncated)
/// 2. Knowledge section (truncated only if User exceeds budget)
/// 3. Procedures section (truncated after Knowledge)
fn truncate_with_priority(content: &str, max_chars: usize) -> String {
    let sections = parse_sections(content);

    let user_len = sections.get("User").map_or(0, |s| s.chars().count());
    let knowledge_len = sections.get("Knowledge").map_or(0, |s| s.chars().count());
    let procedures_len = sections.get("Procedures").map_or(0, |s| s.chars().count());

    let mut result = String::new();
    let mut remaining = max_chars;

    // Add User section (always preserve)
    if let Some(user) = sections.get("User") {
        result.push_str("## User\n");
        result.push_str(user);
        remaining = remaining.saturating_sub("## User\n".chars().count() + user_len);
    }

    // Add Knowledge section (truncate if needed)
    if remaining > 0 {
        if let Some(knowledge) = sections.get("Knowledge") {
            let alloc = remaining.min(knowledge_len);
            if knowledge_len <= remaining {
                result.push_str("\n\n## Knowledge\n");
                result.push_str(knowledge);
                remaining =
                    remaining.saturating_sub("\n\n## Knowledge\n".chars().count() + knowledge_len);
            } else {
                let truncated = truncate_chars(knowledge, alloc.saturating_sub(20));
                result.push_str("\n\n## Knowledge\n");
                result.push_str(&truncated);
                remaining = 0;
            }
        }
    }

    // Add Procedures section with whatever budget remains
    if remaining > 0 {
        if let Some(procedures) = sections.get("Procedures") {
            if procedures_len <= remaining {
                result.push_str("\n\n## Procedures\n");
                result.push_str(procedures);
            } else {
                let truncated = truncate_chars(procedures, remaining.saturating_sub(20));
                result.push_str("\n\n## Procedures\n");
                result.push_str(&truncated);
            }
        }
    }

    result
}

/// Parse content into sections by "## " headings.
fn parse_sections(content: &str) -> std::collections::HashMap<String, String> {
    let mut sections: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    let mut current_section = String::new();
    let mut current_name = String::new();

    for line in content.lines() {
        if let Some(rest) = line.strip_prefix("## ") {
            if !current_name.is_empty() {
                sections.insert(current_name.clone(), current_section.trim().to_string());
            }
            current_name = rest.to_string();
            current_section = String::new();
        } else {
            current_section.push_str(line);
            current_section.push('\n');
        }
    }

    if !current_name.is_empty() {
        sections.insert(current_name, current_section.trim().to_string());
    }

    sections
}

/// Truncate string to max characters, trying to cut at sentence boundaries.
fn truncate_chars(s: &str, max_chars: usize) -> String {
    let chars: Vec<char> = s.chars().collect();
    if chars.len() <= max_chars {
        return s.to_string();
    }

    let limit = max_chars;
    for i in (limit.saturating_sub(100)..limit).rev() {
        if i < chars.len() {
            let c = chars[i];
            if c == '.' || c == '!' || c == '?' {
                return chars[..=i].iter().collect();
            }
        }
    }

    chars[..limit].iter().collect()
}

/// Cap `content` to at most `max_tokens` tokens (via `estimate_tokens`).
///
/// Content under the budget passes through unchanged. Over-budget content is
/// truncated to a prefix — the snapshot lists User first (highest priority),
/// then Knowledge, then Procedures, so a prefix cut preserves the important bits.
fn cap_to_token_budget(content: &str, max_tokens: usize) -> String {
    if estimate_tokens(content) <= max_tokens {
        return content.to_string();
    }
    truncate_to_tokens(content, max_tokens)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn create_test_store() -> (MarkdownMemoryStore, TempDir) {
        let dir = TempDir::new().unwrap();
        let store = MarkdownMemoryStore::new(dir.path().to_path_buf());
        store.init().expect("Failed to init test store");
        (store, dir)
    }

    #[test]
    fn test_char_budget_is_8000() {
        assert_eq!(CHAR_BUDGET, 8000);
    }

    /// CJK content balloons under token accounting: 3000 Chinese chars ≈ 5400+
    /// tokens under `estimate_tokens`. A char-only budget lets non-ASCII memory
    /// dominate every prompt, so the snapshot must also enforce a token cap.
    #[test]
    fn cap_to_token_budget_truncates_cjk_under_budget() {
        let big = format!("## Knowledge\n{}\n", "知".repeat(3000));
        let capped = cap_to_token_budget(&big, SNAPSHOT_TOKEN_BUDGET);
        let tokens = crate::agent::tokenizer::estimate_tokens(&capped);
        assert!(
            tokens <= SNAPSHOT_TOKEN_BUDGET,
            "token cap failed: {tokens} > {SNAPSHOT_TOKEN_BUDGET}",
        );
        assert!(
            capped.chars().count() < big.chars().count(),
            "over-budget content should have been truncated",
        );
    }

    /// Small content under the budget passes through unchanged.
    #[test]
    fn cap_to_token_budget_preserves_small_content() {
        let small = "## User\nhello world\n".to_string();
        let capped = cap_to_token_budget(&small, SNAPSHOT_TOKEN_BUDGET);
        assert_eq!(capped, small, "small content must pass through unchanged");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_empty_snapshot() {
        let dir = TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path()).unwrap();
        let store = MarkdownMemoryStore::new(dir.path().to_path_buf());
        assert!(MemorySnapshot::load_opt(&store).is_none());
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_snapshot_with_content() {
        let (store, _dir) = create_test_store();

        store
            .write_file("user", "User prefers dark mode\nUser speaks Chinese\n")
            .await
            .unwrap();

        store
            .write_file(
                "knowledge",
                "## System Resources\n- Device: TestDevice\n\n## Domain Knowledge\n- Location: TestRoom\n",
            )
            .await
            .unwrap();

        let snapshot = MemorySnapshot::load(&store);
        assert!(!snapshot.is_empty());

        let section = snapshot.to_prompt_section();
        assert!(section.contains("<memory-context>"));
        assert!(section.contains("</memory-context>"));
        assert!(section.contains("User prefers dark mode"));
        // Agents and custom files should NOT be in snapshot
        assert!(!section.contains("Agent Experiences"));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_snapshot_only_user_and_knowledge() {
        let (store, _dir) = create_test_store();

        store.write_file("user", "test user").await.unwrap();
        store
            .write_file("knowledge", "test knowledge")
            .await
            .unwrap();

        // Create agent files — should NOT appear in snapshot
        let agents_dir = store.base_path().join("agents");
        std::fs::create_dir_all(&agents_dir).unwrap();
        std::fs::write(agents_dir.join("agent1.md"), "Agent summary").unwrap();

        // Create custom files — should NOT appear in snapshot
        store
            .write_custom_file("test-file", "Custom content")
            .unwrap();

        let snapshot = MemorySnapshot::load(&store);
        let content = &snapshot.content;

        assert!(content.contains("test user"));
        assert!(content.contains("test knowledge"));
        assert!(!content.contains("Agent summary"));
        assert!(!content.contains("Custom content"));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_priority_truncation_preserves_user_at_all_costs() {
        let (store, _dir) = create_test_store();

        let user = "u".repeat(2000);
        let knowledge = "k".repeat(3000);

        store.write_file("user", &user).await.unwrap();
        store.write_file("knowledge", &knowledge).await.unwrap();

        let snapshot = MemorySnapshot::load(&store);

        // Should be within budget
        assert!(snapshot.content.chars().count() <= CHAR_BUDGET);

        // User should be preserved (first 2000+ chars)
        assert!(snapshot.content.contains(&"u".repeat(100)));
    }

    #[test]
    fn test_truncate_chars() {
        let long = "a".repeat(1000);
        let truncated = truncate_chars(&long, 100);
        assert!(truncated.chars().count() <= 100);
    }

    #[test]
    fn test_truncate_chars_at_sentence() {
        let text = "First sentence. Second sentence. Third sentence.";
        let truncated = truncate_chars(text, 30);
        assert!(truncated.contains("First sentence."));
        assert!(!truncated.contains("Second sentence."));
    }

    #[test]
    fn test_parse_sections() {
        let content = "## User\nuser content\n\n## Knowledge\nknowledge content";
        let sections = parse_sections(content);

        assert_eq!(sections.get("User").unwrap(), "user content");
        assert_eq!(sections.get("Knowledge").unwrap(), "knowledge content");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_snapshot_load_opt() {
        let dir = TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path()).unwrap();
        let store = MarkdownMemoryStore::new(dir.path().to_path_buf());
        assert!(MemorySnapshot::load_opt(&store).is_none());

        let (store, _dir) = create_test_store();
        assert!(MemorySnapshot::load_opt(&store).is_some());
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_char_budget_truncation() {
        let (store, _dir) = create_test_store();

        let user = "u".repeat(2000);
        let knowledge = "k".repeat(3000);

        store.write_file("user", &user).await.unwrap();
        store.write_file("knowledge", &knowledge).await.unwrap();

        let snapshot = MemorySnapshot::load(&store);
        assert!(snapshot.content.chars().count() <= CHAR_BUDGET);
    }

    /// Verifies the truncation path is exercised when all three sections
    /// sit at their per-file maxima. Storage caps each file individually
    /// (user=2000, knowledge=3000, procedures=3000) for a combined body of
    /// 8000 chars. With section headers the combined snapshot slightly
    /// exceeds CHAR_BUDGET, so Procedures (lowest priority) gets trimmed.
    ///
    /// This is the ONLY truncation path reachable through the public
    /// `write_file` API — user-alone-exceeds-budget is impossible because
    /// user is capped at 2000 < CHAR_BUDGET.
    #[tokio::test(flavor = "multi_thread")]
    async fn test_over_budget_truncates_procedures_after_knowledge() {
        let (store, _dir) = create_test_store();

        // Max each file. 2000 + 3000 + 3000 = 8000 body chars; combined
        // with `## User\n\n## Knowledge\n\n## Procedures\n` headers this
        // tips just past the 8000-char budget.
        let user = "U".repeat(2000);
        let knowledge = "K".repeat(3000);
        let procedures = "P".repeat(3000);

        store.write_file("user", &user).await.unwrap();
        store.write_file("knowledge", &knowledge).await.unwrap();
        store.write_file("procedures", &procedures).await.unwrap();

        let snapshot = MemorySnapshot::load(&store);

        // Hard invariant: never over budget.
        assert!(
            snapshot.content.chars().count() <= CHAR_BUDGET,
            "snapshot must fit CHAR_BUDGET (got {}, budget {})",
            snapshot.content.chars().count(),
            CHAR_BUDGET
        );

        // Priority order — User and Knowledge are preserved in full.
        // (Substring of 100 chars is robust to the single 'U' in the
        // `## User` header that would throw off a raw char count.)
        assert!(
            snapshot.content.contains(&"U".repeat(100)),
            "User section must be fully preserved"
        );
        assert!(
            snapshot.content.contains(&"K".repeat(100)),
            "Knowledge section must be fully preserved"
        );

        // Procedures is the lowest-priority section, so it must be the one
        // that gets truncated. Header stays, some `P`s survive, but fewer
        // than the original 3000.
        assert!(
            snapshot.content.contains("## Procedures"),
            "Procedures section header must be preserved even when truncated"
        );
        // Count only body P's (subtract the one in the "## Procedures" header).
        let p_count = snapshot
            .content
            .chars()
            .filter(|c| *c == 'P')
            .count()
            .saturating_sub(1);
        assert!(
            p_count < 3000,
            "Procedures must be truncated (got {} P chars, expected <3000)",
            p_count
        );
        assert!(
            p_count > 0,
            "some Procedures content should survive truncation"
        );
    }

    /// Unit-test `truncate_with_priority` directly against aggressive
    /// over-budget synthetic content. The storage-layer per-file caps make
    /// such content impossible through the public API, but the truncation
    /// function itself must still be robust — it's a defense-in-depth layer
    /// guarding CHAR_BUDGET.
    #[test]
    fn test_truncate_with_priority_aggressive_over_budget() {
        // 3000 (User) + 3000 (Knowledge) + 5000 (Procedures) = 11000 body
        // chars vs 8000-char budget. User + Knowledge fit; Procedures is
        // the section that actually gets truncated here.
        let content = format!(
            "## User\n{}\n\n## Knowledge\n{}\n\n## Procedures\n{}",
            "U".repeat(3000),
            "K".repeat(3000),
            "P".repeat(5000)
        );

        let truncated = truncate_with_priority(&content, CHAR_BUDGET);

        assert!(
            truncated.chars().count() <= CHAR_BUDGET,
            "truncate_with_priority must respect budget (got {}, budget {})",
            truncated.chars().count(),
            CHAR_BUDGET
        );

        // User preserved in full (priority 1). 100-char substring is robust
        // to the single 'U' in the `## User` header.
        assert!(
            truncated.contains(&"U".repeat(100)),
            "User section must be fully preserved"
        );
        // Knowledge preserved in full (priority 2).
        assert!(
            truncated.contains(&"K".repeat(100)),
            "Knowledge section must be fully preserved"
        );
        // Procedures truncated, but header and some content survive.
        assert!(
            truncated.contains("## Procedures"),
            "Procedures section header must be preserved"
        );
        // Count body P's (subtract 1 for the header letter).
        let p_count = truncated.matches('P').count().saturating_sub(1);
        assert!(
            p_count < 5000,
            "Procedures must be truncated (got {}, expected <5000)",
            p_count
        );
        assert!(
            p_count > 0,
            "some Procedures content should survive truncation"
        );
    }
}
