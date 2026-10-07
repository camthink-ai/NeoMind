//! The explore subagent — heavy multi-round lookup in an ISOLATED context.
//!
//! Exploratory questions ("did the cold room misbehave last week?", "find the
//! screenshot from the loading dock") need a dozen tool calls. Run in the main
//! session, those rounds eat the token budget with search noise and force the
//! compactor to summarize away the very clues it found. The explore tool runs
//! a mini ReAct loop in its own context and returns ONLY a compact summary —
//! the main conversation never sees the intermediate rounds.
//!
//! ## The seven state contracts (design freeze, 2026-09-29 + 10-01 corrections)
//!
//! 1. **Self-contained question** — the caller's question must carry all
//!    context; the subagent starts from a blank slate.
//! 2. **Artifacts by path** — images/files the loop produces are referenced by
//!    path in the summary, never inlined into the main context.
//! 3. **Cancellation by drop** — abandoning the tool future tears down the
//!    whole loop; no detached tasks survive (the wall clock is the only
//!    safety net beyond that).
//! 4. **Failure is one-way** — a subagent error affects this call's result
//!    only; no state leaks into the main session.
//! 5. **Read-only** — write-capable tools are removed from the subagent's
//!    catalog (structural), not merely discouraged by prompt. `explore`
//!    itself is excluded to prevent recursion.
//! 6. **Budget as contract** — 8 rounds + 300s wall clock; exhaustion returns
//!    a best-effort summary of what was explored, NOT an error.
//! 7. **Referenceable results** — paths quoted in the summary must be real
//!    tool-artifact paths; fabrication is called out in the system prompt as
//!    the one hard prohibition.
//!
//! ## Concurrency
//!
//! Single-flight, process-wide: one explore at a time. Edge boxes have one
//! inference pipeline — two concurrent explorations would starve everything
//! else. Callers beyond the first wait on the semaphore (the wall clock
//! starts AFTER acquisition, so a queued call still gets its full budget).

use std::sync::Arc;

use neomind_core::llm::backend::{GenerationParams, LlmInput, LlmRuntime, ToolDefinition};
use neomind_core::message::{Content, Message, MessageRole};
use neomind_core::tools::ToolCategory;
use parking_lot::RwLock;
use serde_json::Value;

use crate::toolkit::error::{Result, ToolError};
use crate::toolkit::tool::{Tool, ToolOutput};

/// Round budget: the loop's hard ceiling (contract ⑥).
pub(crate) const MAX_ROUNDS: usize = 8;

/// Wall-clock budget — derived from the SLOW tier (single source of truth
/// in timeouts.rs), NOT a local constant: an explore registered at DEFAULT
/// (~120s) would be killed by the outer tool timeout mid-loop while rounds
/// remain (the §2.0a trap). The ordering assertions below make that drift
/// a compile-time test failure instead of a production surprise.
pub(crate) const WALL_CLOCK: std::time::Duration = crate::toolkit::timeouts::SLOW;

/// Process-wide single-flight permit (contract: one explore at a time).
static EXPLORE_SEMAPHORE: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(1);

/// Tool names the subagent must NEVER see (contract ⑤ + recursion guard).
/// `explore` excludes itself; the rest either mutate system state or are
/// meaningless (and stalling) in an isolated subagent context. Reviewed
/// against the full production registry: shell/file_write/file_edit/memory
/// mutate; run_now spawns agent runs; skill is CRUD; image_edit writes
/// files; ask_user/confirm_action/clarify_intent BLOCK on user interaction
/// that can never arrive — an explore round that calls one burns its wall
/// clock waiting. Extension tools are kept: their value IS the point, and
/// the extension runtime enforces its own command semantics.
const EXCLUDED_TOOLS: &[&str] = &[
    "explore",
    "shell",
    "file_write",
    "file_edit",
    "memory",
    "message",
    "run_now",
    "skill",
    "image_edit",
    "ask_user",
    "confirm_action",
    "clarify_intent",
    "update_task_card", // the subagent must not write the PARENT's card —
                        // its findings reach the card via the parent model
                        // reading the explore result, not by the subagent
                        // clobbering the pinned params directly
];

/// Progress payload for the live panel (§2.0d). Keys are camelCase on the
/// wire, matching the AgentEvent per-field rename convention (`elapsedMs`).
/// Consumers (executor wiring + AgentEvent relay) land with the frontend
/// panel commit on this branch.
impl ExploreProgress {
    /// The wire payload for AgentEvent::Progress.detail (§2.0d): camelCase
    /// keys per the enum's per-field rename convention — the frontend panel
    /// type maps 1:1.
    #[allow(dead_code)] // consumer lands with the stream-relay commit
    pub fn to_detail(&self) -> serde_json::Value {
        serde_json::json!({
            "toolCallId": null, // reserved: the Tool trait carries no call
                                // id today; panels match by stage (single-flight)
            "round": self.round,
            "maxRounds": self.max_rounds,
            "currentTool": self.current_tool,
            "note": self.note,
            "toolCallsSoFar": self.tool_calls_so_far,
            "wallClockMs": self.wall_clock_ms,
        })
    }
}

#[derive(Debug, Clone)]
pub(crate) struct ExploreProgress {
    pub round: usize,
    pub max_rounds: usize,
    pub current_tool: String,
    pub note: String,
    pub tool_calls_so_far: usize,
    pub wall_clock_ms: u64,
}

/// One completed round, kept for the persisted result (F3: the completed
/// panel state survives refresh by living in the tool result itself).
#[derive(Debug, Clone, serde::Serialize)]
pub(crate) struct RoundNote {
    pub tool: String,
    pub note: String,
}

/// Channel for live progress relay. Injected two-phase (the tool is
/// registered before the stream exists); `None` — scheduled agents with no
/// chat stream — means "no relay", silently skipped.
pub(crate) type ProgressSender = Arc<dyn Fn(ExploreProgress) + Send + Sync>;

/// The parent agent's tool whitelist, swapped in per execution (§2.0b:
/// registry ∩ parent's allowed_tools − explore − write tools).
type ScopeHandle = Arc<RwLock<Option<Vec<String>>>>;

/// The explore tool. Holds only injectable handles; all loop state lives in
/// [`run_explore`] so concurrent callers (post-semaphore) never share it.
pub struct ExploreTool {
    /// Backend instance manager (vision-tool pattern): active instance
    /// first, explicit ids fallback. Injected at construction.
    llm_manager: std::sync::Arc<crate::llm_backends::instance_manager::LlmBackendInstanceManager>,
    /// The tool registry, backfilled after registration (chicken-and-egg:
    /// explore must be IN the registry, and needs a handle TO it).
    registry: RwLock<Option<Arc<crate::toolkit::registry::ToolRegistry>>>,
    /// Per-execution parent whitelist (None = unrestricted parent).
    scope: ScopeHandle,
    /// Live progress relay; swapped per execution alongside the scope.
    progress: RwLock<Option<ProgressSender>>,
    /// Test-injected runtime; when set, the manager is bypassed entirely.
    runtime_override: Option<Arc<dyn LlmRuntime>>,
}

impl ExploreTool {
    pub fn new(
        llm_manager: std::sync::Arc<
            crate::llm_backends::instance_manager::LlmBackendInstanceManager,
        >,
    ) -> Self {
        Self {
            llm_manager,
            registry: RwLock::new(None),
            scope: Arc::new(RwLock::new(None)),
            progress: RwLock::new(None),
            runtime_override: None,
        }
    }

    /// Backfill the registry handle (call right after `registry.register`).
    pub fn set_registry(&self, registry: Arc<crate::toolkit::registry::ToolRegistry>) {
        *self.registry.write() = Some(registry);
    }

    /// Test-only: inject a runtime directly, bypassing the instance
    /// manager (which needs a full LlmBackendInstance + storage round-trip
    /// to stand up). Production always goes through `new` + the manager.
    #[cfg(feature = "test-utils")]
    pub fn with_runtime(runtime: Arc<dyn LlmRuntime>) -> Self {
        Self {
            // Placeholder manager (never consulted: runtime_override
            // short-circuits execute()). Unique temp path: parallel tests
            // opening the same ":memory:" file contend on the redb lock.
            llm_manager: std::sync::Arc::new(
                crate::llm_backends::instance_manager::LlmBackendInstanceManager::new(
                    neomind_storage::LlmBackendStore::open(
                        std::env::temp_dir()
                            .join(format!("explore-test-{}.redb", uuid::Uuid::new_v4())),
                    )
                    .unwrap(),
                ),
            ),
            runtime_override: Some(runtime),
            registry: RwLock::new(None),
            scope: Arc::new(RwLock::new(None)),
            progress: RwLock::new(None),
        }
    }

    /// Swap the per-execution SCOPE (the parent agent's tool whitelist).
    /// Called by the executor before every agent execution — INDEPENDENT of
    /// the progress relay, so a chat stream attaching its relay mid-flight
    /// cannot clear a scheduled agent's fence (and vice versa). The old
    /// paired swap was a cross-path clobber: whichever path fired last
    /// zeroed the other's handle.
    pub(crate) fn set_scope(&self, allowed_tools: Option<Vec<String>>) {
        *self.scope.write() = allowed_tools;
    }

    /// Swap the live progress RELAY. Called by the chat stream before a
    /// batch containing explore, and cleared (None) after the batch.
    /// Independent of set_scope for the same cross-path reason.
    pub(crate) fn set_progress(&self, progress: Option<ProgressSender>) {
        *self.progress.write() = progress;
    }

    /// The subagent's tool catalog: the registry minus excluded names, then
    /// intersected with the parent's whitelist when one is active (§2.0b).
    fn subagent_catalog(
        registry: &crate::toolkit::registry::ToolRegistry,
        scope: &Option<Vec<String>>,
    ) -> (
        Vec<ToolDefinition>,
        std::collections::HashMap<String, String>,
    ) {
        // Registry definitions (toolkit type) filtered, then mapped to the
        // backend wire type LlmInput wants. Names sanitized for
        // OpenAI-compatible APIs (extension tools carry `:`/`.`). The
        // reverse map (sanitized → raw) rides alongside: registry.execute
        // expects the RAW name, and the LLM calls by the sanitized one.
        use neomind_core::llm::backend::sanitize_tool_name;
        let mut reverse = std::collections::HashMap::new();
        let catalog: Vec<ToolDefinition> = registry
            .definitions_for_llm()
            .into_iter()
            .filter(|d| !EXCLUDED_TOOLS.contains(&d.name.as_str()))
            .filter(|d| match scope {
                Some(allowed) => allowed.iter().any(|a| a == &d.name),
                None => true,
            })
            .map(|d| {
                let sanitized = sanitize_tool_name(&d.name);
                if sanitized != d.name {
                    reverse.insert(sanitized.clone(), d.name.clone());
                }
                ToolDefinition {
                    name: sanitized,
                    description: d.description.clone(),
                    parameters: d.parameters.clone(),
                }
            })
            .collect();
        (catalog, reverse)
    }
}

#[async_trait::async_trait]
impl Tool for ExploreTool {
    fn name(&self) -> &str {
        "explore"
    }

    fn description(&self) -> &str {
        "Run a self-contained exploratory lookup in an isolated context. Give it a question that \
         carries ALL needed context (it starts blank and cannot see this conversation); it does up \
         to 8 rounds of read-only tool calls and returns {summary, rounds_used, tool_calls_made, \
         wall_clock_ms, rounds}. Use it when answering needs many lookups (history scans, \
         cross-device checks, finding a specific record) — the intermediate rounds never enter \
         this conversation. Files/images it finds come back as paths in the summary. \
         AFTER explore returns, call update_task_card to record what you found in the findings slot."
    }

    fn parameters(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "question": {
                    "type": "string",
                    "description": "The complete, self-contained question to investigate. Include device ids, time ranges, and any names — the subagent sees nothing else."
                }
            },
            "required": ["question"]
        })
    }

    async fn execute(&self, args: Value) -> Result<ToolOutput> {
        let question = args
            .get("question")
            .and_then(|v| v.as_str())
            .filter(|q| !q.trim().is_empty())
            .ok_or_else(|| {
                ToolError::InvalidArguments(
                    "question is required and must be a non-empty string".into(),
                )
            })?
            .to_string();

        // Resolve injectables up-front (fail fast with readable errors).
        let registry = self.registry.read().clone();
        let Some(registry) = registry else {
            return Err(ToolError::Execution(
                "explore: registry not wired (set_registry never called)".into(),
            ));
        };
        let scope = self.scope.read().clone();
        let (catalog, name_map) = Self::subagent_catalog(&registry, &scope);
        if catalog.is_empty() {
            return Err(ToolError::Execution(
                "explore: no read-only tools available under the current scope".into(),
            ));
        }
        let progress = self.progress.read().clone();

        // Pick a runtime BEFORE the semaphore: an instance-less deployment
        // should fail fast, not after queueing (contract ④ test #4).
        let runtime: Arc<dyn LlmRuntime> = match &self.runtime_override {
            Some(rt) => rt.clone(),
            None => self.llm_manager.get_active_runtime().await.map_err(|e| {
                ToolError::Execution(format!("explore: no LLM backend available ({e})"))
            })?,
        };

        // Single-flight. Permit held for the whole loop.
        let _permit = EXPLORE_SEMAPHORE
            .acquire()
            .await
            .map_err(|_| ToolError::Execution("explore semaphore closed".into()))?;

        run_explore(
            &runtime,
            &registry,
            &catalog,
            &name_map,
            question,
            progress.as_ref(),
        )
        .await
    }

    fn category(&self) -> ToolCategory {
        // Analysis, not System: exploration is data work. Enum unchanged
        // (append-only red line).
        ToolCategory::Analysis
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

/// The mini ReAct loop. All state is local — the caller's (main session's)
/// context is never touched (contract ①④).
async fn run_explore(
    runtime: &Arc<dyn LlmRuntime>,
    registry: &Arc<crate::toolkit::registry::ToolRegistry>,
    catalog: &[ToolDefinition],
    name_map: &std::collections::HashMap<String, String>,
    question: String,
    progress: Option<&ProgressSender>,
) -> Result<ToolOutput> {
    let started = std::time::Instant::now();
    let mut messages: Vec<Message> = Vec::new();
    messages.push(Message::new(
        MessageRole::System,
        Content::Text(system_prompt(catalog)),
    ));
    messages.push(Message::new(MessageRole::User, Content::Text(question)));

    let mut rounds: Vec<RoundNote> = Vec::new();
    let mut tool_calls_made = 0usize;
    // Rounds in which the model actually called tools (the summary-only
    // final round does not count toward the budget usage).
    let mut rounds_used = 0usize;
    let mut exhausted = false;

    for round in 1..=MAX_ROUNDS {
        // Context trim: the subagent's message list grows one assistant +
        // up-to-N tool-result messages per round. On an 8K-token backend,
        // 8 rounds × several 1500-char tool results overflows the window —
        // the backend truncates from the FRONT, eating the system prompt
        // and the original question. Keep the two seed messages + the
        // newest body within budget.
        trim_context(&mut messages);

        // Contract ⑥: the wall clock gates EVERY iteration, including the
        // LLM call itself — a slow backend eating the clock returns the
        // partial summary rather than erroring.
        let elapsed = started.elapsed();
        if elapsed >= WALL_CLOCK {
            exhausted = true;
            break;
        }
        let round_budget = WALL_CLOCK - elapsed;

        let input = LlmInput {
            messages: messages.clone(),
            params: GenerationParams {
                temperature: Some(0.2),
                max_tokens: Some(1024),
                thinking_enabled: Some(false),
                ..Default::default()
            },
            model: None,
            stream: false,
            tools: Some(catalog.to_vec()),
        };

        let output = match tokio::time::timeout(round_budget, runtime.generate(input)).await {
            Ok(Ok(o)) => o,
            Ok(Err(e)) => {
                // Contract ④: a backend failure mid-loop degrades to a
                // partial summary when anything was found, else a readable
                // error — never a panic, never silent.
                if rounds.is_empty() {
                    return Err(ToolError::Execution(format!(
                        "explore: LLM backend failed on round {round}: {e}"
                    )));
                }
                tracing::warn!(round, error = %e, "explore: backend failed mid-loop, returning partial summary");
                exhausted = true;
                break;
            }
            Err(_) => {
                exhausted = true;
                break;
            }
        };

        // Round complete → record + progress relay.
        let note = first_line(&output.text);
        rounds.push(RoundNote {
            tool: "reasoning".to_string(),
            note: note.clone(),
        });

        // Native tool calls → execute them (read-only guaranteed by catalog).
        let calls = output.tool_calls.unwrap_or_default();
        let text_is_empty = output.text.trim().is_empty();
        if calls.is_empty() && !text_is_empty {
            // No tool call + non-empty text = the model is done.
            return Ok(finish(
                output.text,
                rounds,
                tool_calls_made,
                rounds_used,
                started,
                false,
            ));
        }
        if calls.is_empty() && text_is_empty {
            // The narration-collapse analog at the subagent level: the
            // model produced NOTHING (no text, no calls). End with a
            // readable fallback rather than an empty-but-successful
            // result — the caller cannot distinguish "found nothing" from
            // "said nothing" with an empty summary.
            return Ok(finish(
                "The exploration model returned no output; no findings were                  produced. Try rephrasing the question."
                    .to_string(),
                rounds,
                tool_calls_made,
                rounds_used,
                started,
                false,
            ));
        }
        rounds_used += 1;

        messages.push(Message::new(
            MessageRole::Assistant,
            Content::Text(output.text.clone()),
        ));

        for call in &calls {
            let (id, name, arguments) = match parse_tool_call(call) {
                Some(t) => t,
                None => continue,
            };
            // The LLM calls by the sanitized name; the registry indexes by
            // the raw one (extension tools carry `:`/`.`).
            let raw_name = name_map.get(&name).unwrap_or(&name).clone();
            tool_calls_made += 1;

            if let Some(relay) = progress {
                relay(ExploreProgress {
                    round,
                    max_rounds: MAX_ROUNDS,
                    current_tool: name.clone(),
                    note: note.clone(),
                    tool_calls_so_far: tool_calls_made,
                    wall_clock_ms: started.elapsed().as_millis() as u64,
                });
            }

            // Tool timeout: the wall clock caps everything — a hung tool
            // cannot outlive the budget.
            let remaining = WALL_CLOCK.saturating_sub(started.elapsed());
            let result =
                tokio::time::timeout(remaining, registry.execute(&raw_name, arguments)).await;

            let tool_text = match result {
                Ok(Ok(out)) => summarize_output(&out),
                Ok(Err(e)) => format!("error: {e}"),
                Err(_) => {
                    exhausted = true;
                    break;
                }
            };

            rounds.push(RoundNote {
                tool: raw_name.clone(),
                note: first_line(&tool_text),
            });
            let _ = id;

            messages.push(Message::new(
                MessageRole::User,
                Content::Text(format!("[{name} result] {tool_text}")),
            ));
        }
        if exhausted {
            break;
        }
    }

    // Budget exhausted (or loop ended without a no-tool round): ask the
    // model for a closing summary from what it has — one more bounded call.
    let summary = closing_summary(runtime, messages, started, exhausted).await;
    Ok(finish(
        summary,
        rounds,
        tool_calls_made,
        rounds_used,
        started,
        exhausted,
    ))
}

/// One bounded final call for the wrap-up summary (contract ⑥: exhaustion
/// returns a summary, not an error). Falls back to concatenating round
/// notes when the model is unreachable — the caller must get SOMETHING.
async fn closing_summary(
    runtime: &Arc<dyn LlmRuntime>,
    mut messages: Vec<Message>,
    started: std::time::Instant,
    exhausted: bool,
) -> String {
    let remaining = WALL_CLOCK.saturating_sub(started.elapsed());
    if remaining.is_zero() {
        return fallback_summary(exhausted);
    }
    messages.push(Message::new(
        MessageRole::User,
        Content::Text(
            "Budget exhausted. Summarize NOW what you found so far in 2-5 sentences. \
             Quote real file paths you saw in tool results. No new tool calls."
                .to_string(),
        ),
    ));
    let input = LlmInput {
        messages,
        params: GenerationParams {
            temperature: Some(0.2),
            max_tokens: Some(512),
            thinking_enabled: Some(false),
            ..Default::default()
        },
        model: None,
        stream: false,
        tools: None,
    };
    match tokio::time::timeout(remaining, runtime.generate(input)).await {
        Ok(Ok(o)) if !o.text.trim().is_empty() => o.text,
        _ => fallback_summary(exhausted),
    }
}

fn fallback_summary(exhausted: bool) -> String {
    if exhausted {
        "Exploration stopped at the budget limit before a final summary could be \
         produced; see the per-round notes for what was examined."
    } else {
        "Exploration ended; see the per-round notes."
    }
    .to_string()
}

/// Assemble the persisted result (contract + F3: wall clock and per-round
/// notes live in the result so the completed panel state survives refresh).
fn finish(
    summary: String,
    rounds: Vec<RoundNote>,
    tool_calls_made: usize,
    rounds_used: usize,
    started: std::time::Instant,
    exhausted: bool,
) -> ToolOutput {
    ToolOutput::success(serde_json::json!({
        "summary": summary,
        "rounds_used": rounds_used,
        "tool_calls_made": tool_calls_made,
        "wall_clock_ms": started.elapsed().as_millis() as u64,
        "budget_exhausted": exhausted,
        "rounds": rounds,
    }))
}

fn system_prompt(catalog: &[ToolDefinition]) -> String {
    let names: Vec<&str> = catalog.iter().map(|d| d.name.as_str()).collect();
    format!(
        "You are a read-only exploration assistant. Answer the question using ONLY these \
         tools: {}. Rules: (1) READ-ONLY — you cannot create, modify, or delete anything; \
         (2) every file path or image path you mention must come verbatim from a tool \
         result — never invent paths; (3) be economical: each tool call should narrow the \
         answer; (4) when you have enough, reply with a 2-5 sentence summary quoting the \
         key paths/values you found, and make no more tool calls.",
        names.join(", ")
    )
}

fn parse_tool_call(call: &Value) -> Option<(String, String, Value)> {
    let id = call
        .get("id")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    // Name: top-level "name" (Anthropic/our format) or nested
    // "function.name" (OpenAI format) — the old code only checked the
    // top level, so OpenAI-format calls were silently dropped.
    let name = call
        .get("name")
        .and_then(|v| v.as_str())
        .or_else(|| {
            call.get("function")
                .and_then(|f| f.get("name"))
                .and_then(|v| v.as_str())
        })?
        .to_string();
    let arguments = call
        .get("arguments")
        .cloned()
        .or_else(|| {
            call.get("function")
                .and_then(|f| f.get("arguments"))
                .cloned()
        })
        .unwrap_or_else(|| serde_json::json!({}));
    Some((id, name, arguments))
}

/// Compact a tool output for the subagent's context: JSON results are
/// truncated hard — the subagent needs the signal, not the payload.
fn summarize_output(out: &ToolOutput) -> String {
    let s = if out.success {
        serde_json::to_string(&out.data).unwrap_or_default()
    } else {
        format!("error: {}", out.error.as_deref().unwrap_or("unknown"))
    };
    truncate_chars(&s, 1500)
}

fn first_line(s: &str) -> String {
    let l = s.lines().find(|l| !l.trim().is_empty()).unwrap_or("");
    truncate_chars(l.trim(), 120)
}

fn truncate_chars(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let cut: String = s.chars().take(max.saturating_sub(1)).collect();
        format!("{cut}…")
    }
}

#[cfg(all(test, feature = "test-utils"))]
mod tests;

/// Keep the subagent's context bounded: the two seed messages (system
/// prompt + question) always survive; the body is trimmed to the newest
/// messages that fit the budget. Uses the same estimator as the rest of
/// the agent (`agent::tokenizer::estimate_tokens`).
fn trim_context(messages: &mut Vec<Message>) {
    const BODY_TOKEN_BUDGET: usize = 6_000;
    if messages.len() <= 3 {
        return;
    }
    let estimate = |m: &Message| -> usize {
        match &m.content {
            neomind_core::Content::Text(t) => crate::agent::tokenizer::estimate_tokens(t),
            neomind_core::Content::Parts(parts) => parts
                .iter()
                .map(|p| crate::agent::tokenizer::estimate_tokens(&p.to_string()))
                .sum(),
        }
    };
    // Walk from the newest backward until the budget is spent.
    let mut used = 0usize;
    let mut keep_from = messages.len();
    for i in (2..messages.len()).rev() {
        used += estimate(&messages[i]);
        if used > BODY_TOKEN_BUDGET {
            break;
        }
        keep_from = i;
    }
    if keep_from > 2 {
        // Snap to the next Assistant boundary: dropping between an
        // assistant message and its tool results leaves orphaned
        // "[tool result]" User messages the model can't contextualize.
        while keep_from < messages.len() && messages[keep_from].role != MessageRole::Assistant {
            keep_from += 1;
        }
        if keep_from > 2 && keep_from < messages.len() {
            messages.drain(2..keep_from);
        }
    }
}
