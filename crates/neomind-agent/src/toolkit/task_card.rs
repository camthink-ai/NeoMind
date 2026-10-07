//! The task card — actively pinned task state that survives compaction.
//!
//! Born from the watermark incident: the model dropped "厦门市集美区" three
//! times across a long conversation. 1.1.1's compaction governance
//! (structured summary template, last-three-user-messages protection,
//! hard-budget eviction order) is PASSIVE protection; the task card is
//! ACTIVE pinning — at task start, the key state is fixed in context and
//! survives across rounds and across compaction.
//!
//! ## Design (plan §3.2, frozen)
//!
//! - **Storage**: the session's existing `todo` file
//!   (`data/memory/sessions/<id>/todo.md`) — the read/write API is already
//!   in `MarkdownMemoryStore::write_session_file("todo")`.
//! - **Structure**: goal / key parameters (user's exact words) / current
//!   step / open items / findings (from explore).
//! - **Injection**: the streaming context builder prepends the card as an
//!   immutable block AFTER the system prompt, BEFORE any history — never
//!   summarized, never evicted (it's not in the history array at all).
//! - **Update**: tool-style — the model calls `update_task_card` with the
//!   changed slots (deterministic, same philosophy as explore).
//! - **explore bridging**: explore's `{summary}` lands in the "findings"
//!   slot via the tool's own arguments (the main model reads the explore
//!   result and updates the card in the same round).

use serde_json::Value;

use crate::toolkit::error::{Result, ToolError};
use crate::toolkit::tool::{Tool, ToolOutput};
use neomind_core::tools::ToolCategory;

/// The five slots of a task card (plan §3.2 "形态"). All optional — the
/// card starts empty and grows as the task evolves.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct TaskCard {
    /// The task's goal — one sentence, the user's intent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub goal: Option<String>,
    /// Key parameters — PRESERVED USER WORDS (the watermark lesson: the
    /// model dropped the district name; the card keeps the exact string).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub params: Option<String>,
    /// Current step — what the model is doing right now.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub step: Option<String>,
    /// Open items — unresolved questions, pending actions.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub open: Option<String>,
    /// Findings — durable discoveries (explore summaries land here).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub findings: Option<String>,
}

impl TaskCard {
    /// Parse from the todo.md JSON body (empty/invalid → default card).
    pub fn from_markdown(text: &str) -> Self {
        // The card is stored as a fenced JSON block inside the todo file;
        // anything before/after is ignored (human notes coexist).
        let start = text.find("```json");
        let end = text.rfind("```");
        match (start, end) {
            (Some(s), Some(e)) if e > s + 7 => {
                serde_json::from_str(text[s + 7..e].trim()).unwrap_or_default()
            }
            _ => Self::default(),
        }
    }

    /// Serialize to the todo.md storage form.
    pub fn to_markdown(&self) -> String {
        let json = serde_json::to_string_pretty(self).unwrap_or_default();
        format!("<!-- task-card (auto-managed; edit via update_task_card tool) -->\n```json\n{json}\n```\n")
    }

    /// The context-injection form: a compact structured block the model
    /// sees on EVERY round, placed before history and never summarized.
    pub fn to_context_block(&self) -> String {
        if self.is_empty() {
            return String::new();
        }
        let mut parts = Vec::new();
        if let Some(g) = &self.goal {
            parts.push(format!("**目标**: {g}"));
        }
        if let Some(p) = &self.params {
            parts.push(format!("**关键参数**: {p}"));
        }
        if let Some(s) = &self.step {
            parts.push(format!("**当前步骤**: {s}"));
        }
        if let Some(o) = &self.open {
            parts.push(format!("**未决项**: {o}"));
        }
        if let Some(f) = &self.findings {
            parts.push(format!("**已发现**: {f}"));
        }
        format!(
            "\n<task-card>\nThese are the pinned facts of the current task. They are NOT part of \
             the conversation history and are NEVER subject to summarization. The parameters \
             above are the user's exact words — use them verbatim in tool calls.\n{}\n</task-card>\n",
            parts.join("\n")
        )
    }

    pub fn is_empty(&self) -> bool {
        self.goal.is_none()
            && self.params.is_none()
            && self.step.is_none()
            && self.open.is_none()
            && self.findings.is_none()
    }
}

/// The `update_task_card` tool. The model calls this to update any slot;
/// omitted slots are preserved (partial update, not replace-all).
///
/// The tool also emits a Progress event (stage: "taskCard") so the
/// frontend knows to refetch the card — same relay pattern as explore.
pub struct TaskCardTool {
    /// The memory store (session-scoped file I/O).
    store: std::sync::Arc<tokio::sync::RwLock<neomind_storage::MarkdownMemoryStore>>,
    /// The current session's id (swapped per chat session, same pattern as
    /// the memory tool's session handle).
    session_id: parking_lot::RwLock<Option<String>>,
    /// Progress relay for the frontend refetch trigger (§3.2 backend 配套).
    progress: parking_lot::RwLock<Option<crate::toolkit::explore::ProgressSender>>,
    /// The cached context block — refreshed on every card write, read
    /// synchronously by the stream every round (file I/O only on update).
    cached_block: parking_lot::RwLock<String>,
}

impl TaskCardTool {
    pub fn new(
        store: std::sync::Arc<tokio::sync::RwLock<neomind_storage::MarkdownMemoryStore>>,
    ) -> Self {
        Self {
            store,
            session_id: parking_lot::RwLock::new(None),
            progress: parking_lot::RwLock::new(None),
            cached_block: parking_lot::RwLock::new(String::new()),
        }
    }

    /// Synchronously read the cached context block (the stream calls this
    /// every round; the file was read at the last update).
    pub fn cached_context_block(&self) -> Option<String> {
        let block = self.cached_block.read().clone();
        if block.is_empty() {
            None
        } else {
            Some(block)
        }
    }

    /// Set the current session (called by the stream per turn, same
    /// pattern as the memory tool's session handle).
    pub fn set_session(&self, id: Option<String>) {
        *self.session_id.write() = id;
        // Clear the cache — the new session's card will be loaded on the
        // first read_card call (or stay empty if no card exists).
        *self.cached_block.write() = String::new();
    }

    /// Set the progress relay (for the frontend refetch trigger).
    #[allow(dead_code)] // stream wiring lands with the frontend commit
    pub(crate) fn set_progress(&self, sender: Option<crate::toolkit::explore::ProgressSender>) {
        *self.progress.write() = sender;
    }

    /// Read the current card for this session.
    pub async fn read_card(&self) -> Result<TaskCard> {
        let sid = self
            .session_id
            .read()
            .clone()
            .ok_or_else(|| ToolError::Execution("task_card: no session bound".into()))?;
        let store = self.store.read().await;
        let text = store
            .read_session_file(&sid, "todo")
            .await
            .map_err(|e| ToolError::Execution(e.to_string()))?;
        let card = TaskCard::from_markdown(&text);
        *self.cached_block.write() = card.to_context_block();
        Ok(card)
    }

    /// Write the card for this session.
    pub async fn write_card(&self, card: &TaskCard) -> Result<()> {
        let sid = self
            .session_id
            .read()
            .clone()
            .ok_or_else(|| ToolError::Execution("task_card: no session bound".into()))?;
        let store = self.store.write().await;
        store
            .write_session_file(&sid, "todo", &card.to_markdown())
            .await
            .map_err(|e| ToolError::Execution(e.to_string()))?;
        drop(store);
        // Refresh the context cache.
        *self.cached_block.write() = card.to_context_block();
        Ok(())
    }
}

#[async_trait::async_trait]
impl Tool for TaskCardTool {
    fn name(&self) -> &str {
        "update_task_card"
    }

    fn description(&self) -> &str {
        "Update the current task card — pinned facts that survive context compression. \
         Update any slot (goal/params/step/open/findings); omitted slots are preserved. \
         The params slot should carry the user's EXACT words for critical parameters \
         (locations, thresholds, file paths) so they are never lost. The findings slot \
         is where you record what you've discovered (e.g., explore results)."
    }

    fn parameters(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "goal": { "type": "string", "description": "The task's goal (one sentence)" },
                "params": { "type": "string", "description": "Key parameters — the user's EXACT words for critical values (locations, thresholds, names)" },
                "step": { "type": "string", "description": "What you're doing right now" },
                "open": { "type": "string", "description": "Unresolved questions / pending actions" },
                "findings": { "type": "string", "description": "Durable discoveries (explore summaries land here)" }
            }
        })
    }

    async fn execute(&self, args: Value) -> Result<ToolOutput> {
        let mut card = self.read_card().await?;

        // Partial update: only the slots present in the arguments change.
        let mut updated = Vec::new();
        for slot in ["goal", "params", "step", "open", "findings"] {
            if let Some(v) = args.get(slot).and_then(|v| v.as_str()) {
                if !v.trim().is_empty() {
                    match slot {
                        "goal" => card.goal = Some(v.to_string()),
                        "params" => card.params = Some(v.to_string()),
                        "step" => card.step = Some(v.to_string()),
                        "open" => card.open = Some(v.to_string()),
                        "findings" => card.findings = Some(v.to_string()),
                        _ => {}
                    }
                    updated.push(slot);
                }
            }
        }

        if updated.is_empty() {
            return Ok(ToolOutput::success(serde_json::json!({
                "message": "No slots to update (all arguments empty or absent). Card unchanged.",
                "card": card
            })));
        }

        self.write_card(&card).await?;

        // Notify the frontend to refetch.
        if let Some(relay) = self.progress.read().as_ref() {
            relay(crate::toolkit::explore::ExploreProgress {
                round: 0,
                max_rounds: 0,
                current_tool: "update_task_card".into(),
                note: updated.join(","),
                tool_calls_so_far: 0,
                wall_clock_ms: 0,
            });
        }

        Ok(ToolOutput::success(serde_json::json!({
            "message": format!("Task card updated: {}", updated.join(", ")),
            "card": card
        })))
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::System
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

#[cfg(all(test, feature = "test-utils"))]
mod tests {
    use super::*;

    fn temp_store() -> std::sync::Arc<tokio::sync::RwLock<neomind_storage::MarkdownMemoryStore>> {
        let dir = tempfile::tempdir().unwrap();
        let store = neomind_storage::MarkdownMemoryStore::new(dir.path());
        store.init().unwrap();
        // Keep the TempDir alive by leaking it (test-scoped).
        std::mem::forget(dir);
        std::sync::Arc::new(tokio::sync::RwLock::new(store))
    }

    /// Round-trip: card → markdown → card preserves all five slots.
    #[test]
    fn task_card_markdown_round_trip() {
        let card = TaskCard {
            goal: Some("给电度表图片加水印".into()),
            params: Some("厦门市集美区 2026-09-30".into()),
            step: Some("找到图片并添加水印".into()),
            open: Some("需要确认字体".into()),
            findings: Some("图片在 /api/images/abc.png".into()),
        };
        let md = card.to_markdown();
        let back = TaskCard::from_markdown(&md);
        assert_eq!(back.goal, card.goal);
        assert_eq!(back.params, card.params);
        assert_eq!(back.step, card.step);
        assert_eq!(back.open, card.open);
        assert_eq!(back.findings, card.findings);
    }

    /// The watermark regression: the user's exact words ("厦门市集美区")
    /// appear in the context block verbatim — the core value proposition.
    #[test]
    fn context_block_preserves_user_exact_words() {
        let card = TaskCard {
            params: Some("厦门市集美区 精度0.5".into()),
            ..Default::default()
        };
        let block = card.to_context_block();
        assert!(
            block.contains("厦门市集美区"),
            "the exact district name must survive"
        );
        assert!(block.contains("<task-card>"), "must be an XML-tagged block");
        assert!(block.contains("NOT part of the conversation history"));
    }

    /// Empty card → empty block (no injection).
    #[test]
    fn empty_card_produces_empty_block() {
        let card = TaskCard::default();
        assert!(card.to_context_block().is_empty());
        assert!(card.is_empty());
    }

    /// The tool updates only the slots present in the arguments.
    #[tokio::test]
    async fn update_task_card_partial_update() {
        let tool = TaskCardTool::new(temp_store());
        // Simulate a session binding by setting the id.
        tool.set_session(Some("test-session".into()));

        // First update: set goal + params.
        tool.execute(serde_json::json!({
            "goal": "给电度表图片加水印",
            "params": "厦门市集美区"
        }))
        .await
        .unwrap();

        // Second update: only step — goal and params must survive.
        tool.execute(serde_json::json!({ "step": "正在查找图片" }))
            .await
            .unwrap();

        let card = tool.read_card().await.unwrap();
        assert_eq!(card.goal.as_deref(), Some("给电度表图片加水印"));
        assert_eq!(card.params.as_deref(), Some("厦门市集美区"));
        assert_eq!(card.step.as_deref(), Some("正在查找图片"));
        assert!(card.findings.is_none(), "findings untouched by the updates");
    }

    /// Empty arguments → no-op, card unchanged.
    #[tokio::test]
    async fn update_task_card_noop_on_empty() {
        let tool = TaskCardTool::new(temp_store());
        tool.set_session(Some("test-session".into()));
        let out = tool.execute(serde_json::json!({})).await.unwrap();
        assert!(out.success);
        assert!(tool.read_card().await.unwrap().is_empty());
    }

    /// The cached context block refreshes on write — the stream's synchronous
    /// read path sees the updated card without file I/O.
    #[tokio::test]
    async fn cached_block_refreshes_on_write() {
        let tool = TaskCardTool::new(temp_store());
        tool.set_session(Some("test-session".into()));

        assert!(tool.cached_context_block().is_none(), "empty initially");

        tool.execute(serde_json::json!({ "goal": "巡检冷库" }))
            .await
            .unwrap();

        let block = tool
            .cached_context_block()
            .expect("block cached after write");
        assert!(block.contains("巡检冷库"));
        assert!(block.contains("<task-card>"));
    }
}
