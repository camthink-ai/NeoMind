//! Tool execution loop for the agent tool-calling mode.
//!
//! Contains the main tool loop (`run_tool_loop`), deduplication logic,
//! duplicate round detection, and helper functions for building round data.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use neomind_core::llm::backend::{LlmError, LlmRuntime};
use neomind_core::message::{Content, ContentPart, Message, MessageRole};
use neomind_storage::AiAgent;

use super::super::AgentExecutor;
use super::{
    compact, summarize_tool_output, truncate_to, DedupOutcome, RoundData, ToolCallRecord,
    ToolLoopOutput,
};
use crate::agent::streaming::resolve_cached_arguments;
use crate::agent::types::{LargeDataCache, ToolCall};

// ---------------------------------------------------------------------------
// impl AgentExecutor — methods that use &self
// ---------------------------------------------------------------------------

impl AgentExecutor {
    /// Run the tool execution loop for up to `max_rounds` LLM calls.
    pub(crate) async fn run_tool_loop(
        &self,
        agent: &AiAgent,
        registry: &crate::toolkit::registry::ToolRegistry,
        llm_runtime: &Arc<dyn LlmRuntime + Send + Sync>,
        filtered_tools: &[neomind_core::llm::backend::ToolDefinition],
        messages: &mut Vec<Message>,
        execution_id: &str,
        max_rounds: usize, // Made implicitly mutable by continuation mechanism below
        tool_name_map: &std::collections::HashMap<String, String>,
        bound_image: Option<&str>,
    ) -> ToolLoopOutput {
        use crate::agent::tool_parser::parse_tool_calls;
        use neomind_core::llm::backend::{GenerationParams, LlmInput};

        // max_rounds may be extended by the continuation mechanism
        let mut max_rounds = max_rounds;

        // Build reverse map: original_name → sanitized_name
        // Used to convert tool result names back to what the LLM expects
        let original_to_sanitized: std::collections::HashMap<String, String> = tool_name_map
            .iter()
            .map(|(sanitized, original)| (original.clone(), sanitized.clone()))
            .collect();

        let mut all_tool_results: Vec<crate::toolkit::ToolResult> = Vec::new();
        let mut round_data_list: Vec<RoundData> = Vec::new();
        let mut final_text = String::new();
        let mut last_llm_error: Option<LlmError> = None;
        let mut step_num = 1u32;
        // Accumulate skill tool results separately — inject as concise prompt, not full history
        let mut skill_reference = String::new();
        let mut skill_injected = false;

        // Per-execution LargeDataCache. Slimmed tool results store their large/base64
        // strings here under `$cached:<key>` references; when the LLM passes those refs
        // back in subsequent tool calls, `resolve_cached_arguments` below substitutes the
        // full data so image-aware tools (vision/image_edit) receive it transparently.
        // Mirrors the chat-agent streaming layer (stream_core/stream_multimodal).
        let mut large_data_cache = LargeDataCache::new();
        // Seed the cache with the agent's bound device image (if any) so the
        // `$cached:` auto-inject path (`resolve_cached_arguments`) can hand the
        // FULL image to image-shaped tool args — including extension tools
        // (YOLO / grounding) whose `image` arg the LLM cannot fill with real
        // bytes. Without this, extensions receive the LLM's truncated base64
        // fragment and return `null` (task #50).
        if let Some(url) = bound_image {
            large_data_cache.seed_bound_image(url);
        }

        // Cross-round tool deduplication: track tool signatures to avoid re-executing
        // the same tool with the same arguments across rounds.
        let mut all_executed_signatures: HashSet<String> = HashSet::new();
        // Duplicate round detection: track tool signatures per round to detect loops.
        let mut prev_round_tool_names: String = String::new();
        let mut consecutive_duplicate_rounds: usize = 0;

        // Get context window for token-aware compaction
        let context_window = llm_runtime.max_context_length();

        // Continuation mechanism: when LLM is still making tool calls at
        // max_rounds, allow extra rounds (up to MAX_CONTINUATION_ROUNDS)
        // so the agent can finish its work instead of being cut off mid-task.
        const MAX_CONTINUATION_ROUNDS: usize = 10;
        let mut round: usize = 0;

        loop {
            if round >= max_rounds {
                break;
            }
            // Inject accumulated skill reference into system prompt once, after first tool round
            if round > 0 && !skill_reference.is_empty() && !skill_injected {
                if let Some(sys_msg) = messages.first_mut() {
                    sys_msg.content = Content::text(format!(
                        "{}\n\n## Skill Reference\n{}",
                        sys_msg.content.as_text(),
                        skill_reference
                    ));
                }
                skill_injected = true;
            }

            let input = LlmInput {
                messages: messages.clone(),
                params: GenerationParams {
                    temperature: Some(0.7),
                    max_tokens: Some(4000),
                    ..Default::default()
                },
                model: None,
                stream: false,
                tools: Some(filtered_tools.to_vec()),
            };

            self.send_thinking(
                &agent.id,
                execution_id,
                step_num,
                &format!("Tool execution round {} - calling LLM", round + 1),
            )
            .await;
            step_num += 1;

            // Retry transient LLM errors (network, timeout, 429) before giving up.
            // Permanent errors (404/403/model-not-found) fail immediately.
            const MAX_TRANSIENT_RETRIES: u32 = 2;
            // Thinking-capable cloud backends (DashScope qwen3.x-plus et al.)
            // can sit silent for 30+ seconds during the reasoning phase under
            // non-streaming mode, hitting gateway idle timeouts (TCP reset /
            // "error sending request for url"). Route through streaming so
            // bytes flow during reasoning — the default `generate_to_completion`
            // consumes the stream and aggregates into the same `LlmOutput`
            // shape this loop expects. Complements commit c6385169's
            // `enable_thinking` manual knob.
            let use_streaming = llm_runtime.capabilities().thinking_display;
            let output = {
                let mut retries = 0u32;
                let mut result: Option<neomind_core::llm::backend::LlmOutput> = None;
                loop {
                    let generate_result = if use_streaming {
                        llm_runtime.generate_to_completion(input.clone()).await
                    } else {
                        llm_runtime.generate(input.clone()).await
                    };
                    match generate_result {
                        Ok(o) => {
                            result = Some(o);
                            break;
                        }
                        Err(e) => {
                            let is_transient = !e.is_permanent();
                            let round_num = round + 1;
                            let msg_count = messages.len();
                            let has_images = messages.iter().any(|m| {
                                matches!(&m.content, Content::Parts(parts) if parts.iter().any(|p| matches!(p, ContentPart::ImageBase64 { .. } | ContentPart::ImageUrl { .. })))
                            });

                            if is_transient && retries < MAX_TRANSIENT_RETRIES {
                                retries += 1;
                                let delay_ms = 500u64 * 2u64.pow(retries); // 1s, then 2s
                                tracing::warn!(
                                    agent_id = %agent.id,
                                    error = %e,
                                    permanent = false,
                                    retry = retries,
                                    max_retries = MAX_TRANSIENT_RETRIES,
                                    delay_ms,
                                    round = round_num,
                                    "Transient LLM error, retrying after delay"
                                );
                                tokio::time::sleep(std::time::Duration::from_millis(delay_ms))
                                    .await;
                                continue;
                            }

                            tracing::warn!(
                                agent_id = %agent.id,
                                error = %e,
                                permanent = e.is_permanent(),
                                round = round_num,
                                msg_count,
                                has_images,
                                model = %llm_runtime.model_name(),
                                retries_exhausted = retries,
                                "LLM generation failed in tool loop (retries exhausted or permanent error)"
                            );
                            last_llm_error = Some(e);
                            final_text = "LLM generation failed during tool execution.".to_string();
                            break;
                        }
                    }
                }
                result
            };

            // If the inner break (failure) fired, bail out of the tool loop.
            let output = match output {
                Some(o) => o,
                None => break, // LLM generation failed
            };

            // Priority: native tool_calls from API → parse from text → thinking field fallback
            let mut tool_calls = if let Some(ref native) = output.tool_calls {
                if !native.is_empty() {
                    tracing::debug!(
                        agent_id = %agent.id,
                        "Using {} native tool calls from API",
                        native.len()
                    );
                    let converted: Vec<ToolCall> = native
                        .iter()
                        .enumerate()
                        .filter_map(|(i, tc)| {
                            // Try "name" first, then "tool"/"function" for consistency with text parser
                            let name = tc
                                .get("name")
                                .and_then(|v| v.as_str())
                                .or_else(|| tc.get("tool").and_then(|v| v.as_str()))
                                .or_else(|| tc.get("function").and_then(|v| v.as_str()));
                            match name {
                                Some(n) => Some(ToolCall {
                                    name: n.to_string(),
                                    id: tc
                                        .get("id")
                                        .and_then(|v| v.as_str())
                                        .filter(|s| !s.is_empty())
                                        .map(|s| s.to_string())
                                        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string()),
                                    arguments: tc
                                        .get("arguments")
                                        .cloned()
                                        .unwrap_or(serde_json::json!({})),
                                    result: None,
                                    round: None,
                                }),
                                None => {
                                    tracing::warn!(
                                        agent_id = %agent.id,
                                        index = i,
                                        "Dropping native tool call with missing name: {:?}",
                                        tc
                                    );
                                    None
                                }
                            }
                        })
                        .collect();
                    if converted.len() != native.len() {
                        tracing::warn!(
                            agent_id = %agent.id,
                            expected = native.len(),
                            converted = converted.len(),
                            "Some native tool calls were dropped due to missing fields"
                        );
                    }
                    converted
                } else {
                    Vec::new()
                }
            } else {
                // Legacy fallback: parse tool calls from response text
                match parse_tool_calls(&output.text) {
                    Ok((_, calls)) if !calls.is_empty() => calls,
                    _ => {
                        // Main text had no parseable tool calls. Check thinking field
                        // — many models (qwen3, deepseek-r1) embed tool calls there.
                        let mut found = Vec::new();

                        // Try thinking field first (models like qwen3/deepseek-r1)
                        if let Some(ref thinking) = output.thinking {
                            // Check for XML-wrapped tool calls: <tool_calls>...</tool_calls>
                            if let Some(start) = thinking.find("<tool_calls>") {
                                if let Some(end) = thinking.find("</tool_calls>") {
                                    let xml_content = &thinking[start..end + 13];
                                    if let Ok((_, calls)) = parse_tool_calls(xml_content) {
                                        if !calls.is_empty() {
                                            tracing::debug!(
                                                agent_id = %agent.id,
                                                "Found {} tool calls in thinking XML",
                                                calls.len()
                                            );
                                            found.extend(calls);
                                        }
                                    }
                                }
                            }

                            // Also try JSON-style tool calls in thinking
                            if found.is_empty() {
                                if let Ok((_, calls)) = parse_tool_calls(thinking) {
                                    if !calls.is_empty() {
                                        tracing::debug!(
                                            agent_id = %agent.id,
                                            "Found {} tool calls in thinking field (fallback)",
                                            calls.len()
                                        );
                                        found.extend(calls);
                                    }
                                }
                            }
                        }

                        if !found.is_empty() {
                            found
                        } else {
                            // No tool calls found anywhere — LLM produced final text
                            final_text = output.text;
                            break;
                        }
                    }
                }
            };

            // Get remaining text for reasoning tracking
            let remaining_text = if output.tool_calls.is_some() {
                // Native tool calls: strip the appended JSON from text directly
                // (backends append serialized tool_calls to response_text for backward compat)
                if let Some(pos) = output.text.rfind('[') {
                    // Heuristic: if the last '[' starts a valid JSON array that looks like tool calls,
                    // take everything before it as the reasoning text.
                    let candidate = &output.text[pos..];
                    if candidate.starts_with("[{\"") {
                        output.text[..pos].trim().to_string()
                    } else {
                        output.text.clone()
                    }
                } else {
                    output.text.clone()
                }
            } else {
                // Legacy path: parse tool calls from text to extract the non-tool portion
                match parse_tool_calls(&output.text) {
                    Ok((text, _)) => text,
                    Err(_) => output.text.clone(),
                }
            };

            if tool_calls.is_empty() {
                final_text = remaining_text;
                break;
            }

            // --- Per-round tool call cap ---
            // Prevent single-round explosion (e.g. 17 parallel device queries).
            // Keep only the first N calls and tell the LLM to defer the rest.
            const MAX_TOOL_CALLS_PER_ROUND: usize = 6;
            if tool_calls.len() > MAX_TOOL_CALLS_PER_ROUND {
                let total = tool_calls.len();
                let deferred_names: Vec<String> = tool_calls[MAX_TOOL_CALLS_PER_ROUND..]
                    .iter()
                    .map(|tc| {
                        tc.arguments
                            .get("command")
                            .and_then(|v| v.as_str())
                            .unwrap_or(&tc.name)
                            .split_whitespace()
                            .take(4)
                            .collect::<Vec<_>>()
                            .join(" ")
                    })
                    .collect();
                tracing::info!(
                    agent_id = %agent.id,
                    round = round + 1,
                    total,
                    kept = MAX_TOOL_CALLS_PER_ROUND,
                    "Capping tool calls per round"
                );
                tool_calls.truncate(MAX_TOOL_CALLS_PER_ROUND);
                // Inject hint so LLM knows there's more work to do
                messages.push(Message::new(
                    MessageRole::User,
                    Content::text(format!(
                        "[System] {} tool call(s) were deferred to save time. Remaining tasks: {}. \
                         Continue in the next round if needed.",
                        total - MAX_TOOL_CALLS_PER_ROUND,
                        deferred_names.join("; ")
                    )),
                ));
            }

            // --- Intra-round + Cross-round deduplication ---
            let dedup_outcome = deduplicate_tool_calls(
                &mut tool_calls,
                &mut all_executed_signatures,
                &agent.id,
                round,
            );

            if matches!(dedup_outcome, DedupOutcome::AllDuplicate) {
                messages.push(Message::new(
                    MessageRole::Assistant,
                    Content::text(&output.text),
                ));
                messages.push(Message::new(
                    MessageRole::User,
                    Content::text(
                        "Those tool calls were already executed in previous rounds with the same \
                         arguments. Please use different tools or parameters, or provide your \
                         final answer based on the results you already have.",
                    ),
                ));
                continue;
            }

            // --- Partial-dedup hint: some calls were skipped, some survived ---
            if let DedupOutcome::HasNew {
                skipped_cross_round,
            } = &dedup_outcome
            {
                if !skipped_cross_round.is_empty() {
                    let skipped_summary: Vec<String> = skipped_cross_round
                        .iter()
                        .map(|s| s.split_whitespace().take(5).collect::<Vec<_>>().join(" "))
                        .collect();
                    messages.push(Message::new(
                        MessageRole::User,
                        Content::text(format!(
                            "[System] Skipped {} duplicate tool call(s). Commands already executed: {}. \
                             Use the results from previous rounds instead of re-querying.",
                            skipped_cross_round.len(),
                            skipped_summary.join("; ")
                        )),
                    ));
                }
            }

            // --- Duplicate round detection ---
            let should_break = detect_duplicate_round(
                &tool_calls,
                &mut prev_round_tool_names,
                &mut consecutive_duplicate_rounds,
                &agent.id,
                round,
            );
            // We need &self for send_thinking, so handle the break here
            let should_break = if should_break {
                self.send_thinking(
                    &agent.id,
                    execution_id,
                    step_num,
                    "Stopping: detected repeated tool calling pattern, forcing text response",
                )
                .await;
                true
            } else {
                false
            };
            if should_break {
                break;
            }

            tracing::debug!(
                agent_id = %agent.id, round = round + 1, tool_count = tool_calls.len(),
                "Tool calls received"
            );

            self.send_thinking(
                &agent.id,
                execution_id,
                step_num,
                &format!(
                    "Round {}: Executing {} tool(s): {}",
                    round + 1,
                    tool_calls.len(),
                    tool_calls
                        .iter()
                        .map(|tc| tc.name.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
            )
            .await;
            step_num += 1;

            messages.push(Message::new(
                MessageRole::Assistant,
                Content::text(&output.text),
            ));

            // Execute tools with concurrency limiting via semaphore
            // Map sanitized tool names back to original names for registry lookup.
            // Resolve `$cached:<key>` references in tool arguments against this
            // execution's LargeDataCache so image-aware tools receive the full
            // binary payload (the LLM only sees the slim summary in its prompt).
            let calls: Vec<_> = tool_calls
                .iter()
                .map(|tc| {
                    let original_name = tool_name_map
                        .get(&tc.name)
                        .cloned()
                        .unwrap_or_else(|| tc.name.clone());
                    let resolved_args =
                        resolve_cached_arguments(&tc.arguments, &large_data_cache, &original_name);
                    crate::toolkit::registry::ToolCall {
                        name: original_name,
                        args: resolved_args,
                        id: Some(tc.id.clone()),
                    }
                })
                .collect();
            let results = if calls.is_empty() {
                Vec::new()
            } else {
                let _permit = match self.tool_concurrency.acquire().await {
                    Ok(p) => p,
                    Err(e) => {
                        tracing::error!("Tool concurrency semaphore closed: {}", e);
                        break;
                    }
                };
                registry.execute_parallel(calls).await
            };

            let round_tool_calls = build_round_tool_calls(&tool_calls, &results, tool_name_map);

            round_data_list.push(RoundData {
                thought: if remaining_text.is_empty() {
                    None
                } else {
                    Some(remaining_text)
                },
                tool_calls: round_tool_calls,
            });

            let new_step_num = self
                .process_tool_results(
                    &results,
                    messages,
                    &mut all_tool_results,
                    &mut skill_reference,
                    &original_to_sanitized,
                    &agent.id,
                    execution_id,
                    step_num,
                    &mut large_data_cache,
                )
                .await;
            step_num = new_step_num;

            // --- Messages compaction ---
            // When the message history grows too large, compact old tool results into
            // short summaries to prevent context window overflow in subsequent rounds.
            let msg_count_before = messages.len();
            compact::compact_executor_messages(messages, context_window);
            let msg_count_after = messages.len();

            // --- Inject queried-entities summary after compaction ---
            // When compaction removed messages, the LLM may "forget" what it already
            // queried and re-query the same entities. Inject a concise reminder of
            // all executed signatures to prevent redundant queries.
            if msg_count_after < msg_count_before {
                let sig_count = all_executed_signatures.len();
                if sig_count > 0 && sig_count <= 30 {
                    let sigs: Vec<&str> =
                        all_executed_signatures.iter().map(|s| s.as_str()).collect();
                    messages.push(Message::new(
                        MessageRole::User,
                        Content::text(format!(
                            "[System] Context was compacted. You have already executed {} tool call(s) — do NOT re-execute them:\n{}",
                            sig_count,
                            sigs.join("\n")
                        )),
                    ));
                } else if sig_count > 30 {
                    messages.push(Message::new(
                        MessageRole::User,
                        Content::text(format!(
                            "[System] Context was compacted. You have already executed {} tool calls across previous rounds. \
                             Do NOT re-query any entities you have already checked.",
                            sig_count
                        )),
                    ));
                }
            }

            // --- Continuation check ---
            // At the current max_rounds boundary, if the LLM was still making
            // tool calls this round (didn't naturally finish), extend the loop
            // so the agent can complete its work instead of being cut off mid-task.
            let had_tool_calls = !round_data_list
                .last()
                .is_none_or(|rd| rd.tool_calls.is_empty());
            if round + 1 == max_rounds && had_tool_calls {
                let extension = MAX_CONTINUATION_ROUNDS;
                max_rounds += extension;
                tracing::info!(
                    agent_id = %agent.id,
                    new_limit = max_rounds,
                    "LLM still executing tools at round limit — extending by {} rounds",
                    extension,
                );
            }

            round += 1;
        }

        // If all rounds exhausted without LLM producing final text, OR if LLM failed
        // mid-loop (error message in final_text), use Focused's Phase 2 pattern to
        // generate a natural language conclusion.
        let needs_summary = final_text.is_empty()
            || final_text == "LLM generation failed during tool execution."
            || final_text == "Completed tool execution rounds.";
        if needs_summary && !all_tool_results.is_empty() {
            final_text.clear();
            let summary = self
                .generate_phase2_summary(
                    agent,
                    llm_runtime,
                    &all_tool_results,
                    round_data_list.len(),
                )
                .await;
            if let Some(text) = summary {
                final_text = text;
            } else {
                // Phase 2 LLM call failed — build a concise fallback from tool results
                // instead of returning a generic "Completed" message that loses all data.
                let success_count = all_tool_results.iter().filter(|r| r.result.is_ok()).count();
                let total_count = all_tool_results.len();
                let mut lines = vec![format!(
                    "Tool execution completed: {}/{} calls succeeded across {} round(s).",
                    success_count,
                    total_count,
                    round_data_list.len()
                )];
                // Include brief summaries of last few successful results
                for r in all_tool_results.iter().rev().take(5) {
                    if let Ok(ref output) = r.result {
                        let brief = summarize_tool_output(&output.data, &r.name);
                        lines.push(format!("- [{}] {}", r.name, truncate_to(&brief, 200)));
                    }
                }
                final_text = lines.join("\n");
            }
        }

        if final_text.is_empty() {
            final_text = "Completed tool execution rounds.".to_string();
        }

        ToolLoopOutput {
            final_text,
            all_tool_results,
            round_data_list_raw: round_data_list
                .into_iter()
                .map(|rd| (rd.thought, rd.tool_calls))
                .collect(),
            last_llm_error,
        }
    }
}

// ---------------------------------------------------------------------------
// Free functions (no &self)
// ---------------------------------------------------------------------------

/// Intra-round and cross-round deduplication of tool calls.
///
/// Removes duplicate tool calls within the same round (same name + similar args),
/// then filters out tool calls that were already executed in previous rounds.
/// Returns whether all tool calls were filtered out (all duplicates).
pub(crate) fn deduplicate_tool_calls(
    tool_calls: &mut Vec<ToolCall>,
    all_executed_signatures: &mut HashSet<String>,
    agent_id: &str,
    round: usize,
) -> DedupOutcome {
    // --- Intra-round deduplication ---
    let mut seen_this_round: HashSet<String> = HashSet::new();
    tool_calls.retain(|tc| {
        let sig = tool_signature(tc);
        seen_this_round.insert(sig)
    });

    // --- Cross-round deduplication ---
    let before_count = tool_calls.len();
    let mut skipped_cross_round: Vec<String> = Vec::new();
    tool_calls.retain(|tc| {
        let sig = tool_signature(tc);
        if all_executed_signatures.contains(&sig) {
            // Collect a human-readable summary for the hint
            if tc.name == "shell" {
                if let Some(cmd) = tc.arguments.get("command").and_then(|v| v.as_str()) {
                    skipped_cross_round.push(cmd.to_string());
                }
            } else {
                skipped_cross_round.push(sig);
            }
            false
        } else {
            all_executed_signatures.insert(sig);
            true
        }
    });
    let deduped_count = before_count - tool_calls.len();
    if deduped_count > 0 {
        tracing::debug!(
            agent_id = %agent_id,
            round = round + 1,
            deduped = deduped_count,
            "Skipped duplicate tool calls from previous rounds"
        );
    }

    if tool_calls.is_empty() {
        tracing::warn!(
            agent_id = %agent_id,
            round = round + 1,
            "All tool calls were duplicates, asking LLM to proceed differently"
        );
        DedupOutcome::AllDuplicate
    } else {
        DedupOutcome::HasNew {
            skipped_cross_round,
        }
    }
}

/// Compute a dedup-signature for a tool call.
///
/// For the `shell` tool, normalizes the command (strips cosmetic flags,
/// collapses whitespace) and ignores the `description` field entirely,
/// so that re-querying the same device with a different description
/// still counts as a duplicate.
///
/// For all other tools, falls back to `name:first_100_chars_of_args_json`.
pub(crate) fn tool_signature(tc: &ToolCall) -> String {
    if tc.name == "shell" {
        let command = tc
            .arguments
            .get("command")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let normalized = normalize_shell_command(command);
        format!("shell:{}", normalized)
    } else {
        let args_preview = serde_json::to_string(&tc.arguments).unwrap_or_default();
        let bound = args_preview.len().min(100);
        let args_short = &args_preview[..args_preview.floor_char_boundary(bound)];
        format!("{}:{}", tc.name, args_short)
    }
}

/// Normalize a neomind CLI command for dedup purposes:
/// collapse whitespace, strip cosmetic flags, and collapse entity-specific
/// sub-commands so that re-querying the same entity counts as a duplicate.
///
/// Only applies entity-level truncation for `get` actions (which return the same
/// entity data regardless of trailing words). Other actions like `history`,
/// `execute`, `list` are kept in full to preserve meaningful parameter differences.
///
/// Examples:
///   `neomind device get abc123 --format json`  -> `neomind device get abc123`
///   `neomind device get abc123 battery metrics` -> `neomind device get abc123`
///   `neomind device history abc123 --time-range 7d` -> `neomind device history abc123 --time-range 7d`
pub(crate) fn normalize_shell_command(cmd: &str) -> String {
    let parts: Vec<&str> = cmd.split_whitespace().collect();
    if parts.is_empty() {
        return String::new();
    }

    // Check if this is a `neomind <domain> <action>` with action safe to truncate
    let action_safe_to_truncate =
        parts.len() >= 3 && parts[0] == "neomind" && matches!(parts[2], "get");

    let mut filtered = Vec::new();
    let mut skip_next = false;
    for (i, part) in parts.iter().enumerate() {
        if skip_next {
            skip_next = false;
            continue;
        }
        // Strip cosmetic flags that don't change the query result:
        // --format and --output only affect presentation, not data.
        // NOTE: --limit, --time-range, --offset etc. are NOT stripped because
        // they change the actual data returned.
        if *part == "--format" || *part == "--output" {
            skip_next = true;
            continue;
        }
        if part.starts_with("--format=") || part.starts_with("--output=") {
            continue;
        }
        filtered.push(*part);

        // Entity-level dedup for `get` actions: after `neomind <domain> get <id>`,
        // stop collecting. Extra words like "battery" or "metrics" are just
        // LLM-added hints — `device get abc123` returns all data regardless.
        // Only applies to `get` — `history`/`execute`/`list` keep full args.
        if action_safe_to_truncate && i >= 3 && filtered.len() >= 4 {
            break;
        }
    }
    filtered.join(" ")
}

/// Detect duplicate rounds by comparing tool signatures.
///
/// Compares tool signatures (name + key arguments) to detect truly stuck loops.
/// Only counts as duplicate when the FULL round's tool set AND arguments match
/// the previous round — different arguments to the same tool are NOT duplicates.
///
/// Returns `true` if the LLM is stuck (3+ consecutive identical rounds).
pub(crate) fn detect_duplicate_round(
    tool_calls: &[ToolCall],
    prev_round_tool_names: &mut String,
    consecutive_duplicate_rounds: &mut usize,
    agent_id: &str,
    round: usize,
) -> bool {
    let current_round_sig = {
        let mut sigs: Vec<String> = tool_calls
            .iter()
            .map(|tc| {
                let action = tc
                    .arguments
                    .get("action")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let mut sig = format!("{}|{}", tc.name, action);
                // Include shell command so different commands don't look identical
                if let Some(cmd) = tc.arguments.get("command").and_then(|v| v.as_str()) {
                    sig.push_str(&format!("|cmd:{}", cmd));
                }
                for param in &["device_id", "metric", "agent_id", "rule_id", "extension_id"] {
                    if let Some(val) = tc.arguments.get(*param).and_then(|v| v.as_str()) {
                        sig.push_str(&format!("|{}", val));
                    }
                }
                sig
            })
            .collect();
        sigs.sort();
        sigs.join(";;")
    };
    if current_round_sig == *prev_round_tool_names {
        *consecutive_duplicate_rounds += 1;
        tracing::info!(
            agent_id = %agent_id,
            round = round + 1,
            consecutive_duplicates = consecutive_duplicate_rounds,
            "Duplicate tool round detected (same tools + args) — continuing, cross-round dedup handles re-execution"
        );
    } else {
        *consecutive_duplicate_rounds = 0;
    }
    *prev_round_tool_names = current_round_sig;

    // Stop after 3+ consecutive identical rounds — the LLM is stuck.
    // Repeated tool calls in complex tasks are normal; cross-round dedup above
    // already prevents actual re-execution.
    if *consecutive_duplicate_rounds >= 3 {
        tracing::warn!(
            agent_id = %agent_id,
            round = round + 1,
            consecutive_duplicates = consecutive_duplicate_rounds,
            "LLM stuck in loop (3+ consecutive duplicate rounds), forcing text response"
        );
        true
    } else {
        false
    }
}

/// Build the list of ToolCallRecords from executed tool calls and their results.
pub(crate) fn build_round_tool_calls(
    tool_calls: &[ToolCall],
    results: &[crate::toolkit::ToolResult],
    tool_name_map: &HashMap<String, String>,
) -> Vec<ToolCallRecord> {
    let mut round_tool_calls: Vec<ToolCallRecord> = Vec::new();
    for (i, tc) in tool_calls.iter().enumerate() {
        let result = results
            .get(i)
            .cloned()
            .unwrap_or_else(|| crate::toolkit::ToolResult {
                name: tool_name_map
                    .get(&tc.name)
                    .cloned()
                    .unwrap_or_else(|| tc.name.clone()),
                result: Err(crate::toolkit::error::ToolError::Execution(
                    "No result".to_string(),
                )),
            });
        // Use original name for history display
        let display_name = tool_name_map
            .get(&tc.name)
            .cloned()
            .unwrap_or_else(|| tc.name.clone());
        round_tool_calls.push(ToolCallRecord {
            name: display_name,
            input: tc.arguments.clone(),
            result,
        });
    }
    round_tool_calls
}
