//! Tool execution loop for the agent tool-calling mode.
//!
//! Contains the bounded tool loop and helpers for preserving execution evidence.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use neomind_core::llm::backend::{LlmError, LlmRuntime};
use neomind_core::message::{Content, ContentPart, Message, MessageRole};
use neomind_storage::AiAgent;

use super::super::AgentExecutor;
use super::{
    compact, summarize_tool_output, truncate_to, RoundData, StopReason, ToolCallRecord,
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
        llm_runtime: &Arc<dyn LlmRuntime>,
        filtered_tools: &[neomind_core::llm::backend::ToolDefinition],
        messages: &mut Vec<Message>,
        execution_id: &str,
        max_rounds: usize,
        tool_name_map: &std::collections::HashMap<String, String>,
        bound_image: Option<&str>,
    ) -> ToolLoopOutput {
        use neomind_core::llm::backend::{GenerationParams, LlmInput};

        // Build reverse map: original_name → sanitized_name
        // Used to convert tool result names back to what the LLM expects
        let original_to_sanitized: std::collections::HashMap<String, String> = tool_name_map
            .iter()
            .map(|(sanitized, original)| (original.clone(), sanitized.clone()))
            .collect();

        let mut all_tool_results: Vec<crate::toolkit::ToolResult> = Vec::new();
        let mut round_data_list: Vec<RoundData> = Vec::new();
        let mut final_text = String::new();
        // Why the loop ended — set at every break, surfaced via ToolLoopOutput.
        let mut stop_reason = StopReason::NaturalCompletion;
        let mut last_llm_error: Option<LlmError> = None;
        let mut step_num = 1u32;
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

        // Get context window for token-aware compaction
        let context_window = llm_runtime.max_context_length();

        // Per-run token budget accumulator (backend-reported usage only —
        // see ToolLoopOutput::tokens_used). Checked after every LLM call.
        let token_budget = agent.max_tokens_per_run;
        let mut tokens_used: u64 = 0;

        let mut round: usize = 0;

        loop {
            // Agent defaults: read ONCE — AgentDefaults::get() opens the
            // settings DB on every call and this loop used to do that twice
            // per round (60 DB opens on a 30-round execution).
            let agent_defaults = neomind_storage::AgentDefaults::get();
            if round >= max_rounds {
                tracing::info!(
                    agent_id = %agent.id,
                    max_rounds,
                    "Reached round budget — breaking to Phase 2 summary"
                );
                stop_reason = StopReason::MaxRounds;
                break;
            }

            let input = LlmInput {
                messages: messages.clone(),
                params: GenerationParams {
                    // AgentDefaults is the /api/settings/agent surface — the
                    // loop used to hardcode 0.7 and ignore it (config only
                    // fed the chat path).
                    temperature: Some(agent_defaults.default_temperature),
                    top_p: Some(agent_defaults.default_top_p),
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
                // Context overflow is permanent per `is_permanent()`, but on
                // local backends (llama.cpp/Ollama) a window SMALLER than the
                // registry default means EVERY round overflows. One hard-
                // compaction retry turns "small-model execution inevitably
                // fails" into "completes"; only give up if even the halved
                // window still overflows.
                let mut overflow_retried = false;
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
                            let is_overflow = matches!(&e, LlmError::ContextOverflow { .. });
                            let is_transient =
                                !e.is_permanent() || (is_overflow && !overflow_retried);
                            let round_num = round + 1;
                            let msg_count = messages.len();
                            let has_images = messages.iter().any(|m| {
                                matches!(&m.content, Content::Parts(parts) if parts.iter().any(|p| matches!(p, ContentPart::ImageBase64 { .. } | ContentPart::ImageUrl { .. })))
                            });

                            if is_transient && retries < MAX_TRANSIENT_RETRIES {
                                retries += 1;
                                if is_overflow {
                                    overflow_retried = true;
                                    // Shrink harder than the per-round pass:
                                    // halve the effective window so
                                    // CompactionConfig keeps fewer full-size
                                    // results and evicts more aggressively.
                                    let window =
                                        if context_window == 0 || context_window > 1_000_000 {
                                            8192
                                        } else {
                                            context_window
                                        };
                                    compact::compact_executor_messages(messages, window / 2);
                                    tracing::warn!(
                                        agent_id = %agent.id,
                                        round = round_num,
                                        "Context overflow — hard-compacting messages (halved window) and retrying once"
                                    );
                                }
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
                            stop_reason = StopReason::LlmError;
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

            // --- Per-run token budget ---
            // Round/timeout/call caps count the wrong units for cost; this is
            // the only guardrail in tokens. Phase 2 below still runs on a
            // budget stop (one bounded call) so the run produces a usable
            // conclusion instead of dying mid-thought — same shape as the
            // max-rounds stop.
            if let Some(usage) = &output.usage {
                tokens_used += usage.total_tokens as u64;
            }
            if let Some(cap) = token_budget {
                if tokens_used >= cap {
                    tracing::warn!(
                        agent_id = %agent.id,
                        tokens_used,
                        cap,
                        round = round + 1,
                        "Per-run token budget exceeded — stopping tool loop"
                    );
                    stop_reason = StopReason::TokenBudget;
                    break;
                }
            }

            let tool_calls = crate::agent::tool_parser::native_tool_calls(
                output.tool_calls.clone().unwrap_or_default(),
            );
            let remaining_text = output.text.clone();
            if tool_calls.is_empty() {
                final_text = remaining_text;
                break;
            }

            // --- Per-round tool call cap ---
            // Prevent single-round explosion (e.g. 17 parallel device queries).
            // Execute ALL calls the model asked for (the executor semaphore +
            // per-batch concurrency below bound in-flight work) — truncating
            // here would silently drop calls the model explicitly wanted. Only
            // log the batch size for observability.
            const MAX_TOOL_CALLS_PER_ROUND: usize = 6;
            if tool_calls.len() > MAX_TOOL_CALLS_PER_ROUND {
                tracing::info!(
                    agent_id = %agent.id,
                    round = round + 1,
                    total = tool_calls.len(),
                    "Executing full tool-call batch"
                );
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

            // Calls have a distinct output channel. Keep their arguments in
            // history as an explicit factual record, including tool-only replies.
            let call_record = serde_json::to_string(&output.tool_calls).unwrap_or_default();
            messages.push(Message::new(
                MessageRole::Assistant,
                Content::text(format!(
                    "{}\n[Tool calls recorded: {}]",
                    output.text, call_record
                )),
            ));

            // Execute tools with concurrency limiting via semaphore
            // Map sanitized tool names back to original names for registry lookup.
            // Resolve `$cached:<key>` references in tool arguments against this
            // execution's LargeDataCache so image-aware tools receive the full
            // binary payload (the LLM only sees the slim summary in its prompt).
            //
            // Hallucinated CLI-domain tools → shell: weak models sometimes emit
            // a whole `neomind ...` command or a CLI domain (e.g. `device(...)`,
            // `rule(...)`) as the tool name instead of calling `shell`. The chat
            // path auto-routes these to `shell` (tool_exec.rs) before executing;
            // the scheduled path previously only emitted a text hint and burned
            // an extra LLM round re-emitting the same call. Mirror the chat
            // behavior here: resolve through the shared mapper, and when it maps
            // to `shell`, convert the structured args into the CLI command string
            // `ShellTool` expects ({"command": "neomind <domain> ..."}).
            let calls: Vec<_> = tool_calls
                .iter()
                .map(|tc| {
                    let original_name = tool_name_map
                        .get(&tc.name)
                        .cloned()
                        .unwrap_or_else(|| tc.name.clone());
                    let (exec_name, exec_args) = if original_name == "shell" {
                        // Real shell call — pass through, resolve $cached refs as-is.
                        (
                            original_name.clone(),
                            resolve_cached_arguments(
                                &tc.arguments,
                                &large_data_cache,
                                &original_name,
                            ),
                        )
                    } else if crate::tools::resolve_tool_name(&original_name) == "shell"
                        && original_name != "shell"
                    {
                        // Hallucinated CLI-domain tool → convert to a shell command.
                        match crate::tools::mapper::build_cli_command(&original_name, &tc.arguments)
                        {
                            Some(args) => ("shell".to_string(), args),
                            None => (
                                original_name.clone(),
                                resolve_cached_arguments(
                                    &tc.arguments,
                                    &large_data_cache,
                                    &original_name,
                                ),
                            ),
                        }
                    } else {
                        (
                            original_name.clone(),
                            resolve_cached_arguments(
                                &tc.arguments,
                                &large_data_cache,
                                &original_name,
                            ),
                        )
                    };
                    crate::toolkit::registry::ToolCall {
                        name: exec_name,
                        args: exec_args,
                        id: Some(tc.id.clone()),
                    }
                })
                .collect();
            // Execute tools in batches of MAX_TOOL_CALLS_PER_ROUND so a large
            // batch (e.g. 17 device queries) never runs unbounded-parallel
            // (JoinSet spawns everything at once; the semaphore only bounds
            // the batch, not per-tool work). All calls execute — nothing is
            // dropped. Results are reassembled in original order (execute_parallel
            // returns in input order, so positions line up).
            let results = if calls.is_empty() {
                Vec::new()
            } else {
                // Safety policy: block catastrophic shell commands (rm -rf /,
                // dd to a device, mkfs, pipe-to-shell, fork bomb, destructive
                // neomind CLI) BEFORE execution. Denied calls get a synthetic
                // error result at their position; the rest execute normally.
                let blocked: Vec<(usize, String)> = calls
                    .iter()
                    .enumerate()
                    .filter_map(|(i, c)| {
                        if c.name == "shell" {
                            c.args
                                .get("command")
                                .and_then(|v| v.as_str())
                                .and_then(crate::toolkit::policy::deny_reason)
                                .map(|r| (i, r))
                        } else {
                            None
                        }
                    })
                    .collect();

                // Split into blocked + permitted so we can run only permitted calls.
                let blocked_set: HashSet<usize> = blocked.iter().map(|(i, _)| *i).collect();
                let mut assembled: Vec<Option<crate::toolkit::ToolResult>> =
                    (0..calls.len()).map(|_| None).collect();
                let permitted: Vec<(usize, crate::toolkit::registry::ToolCall)> = calls
                    .iter()
                    .enumerate()
                    .filter(|(i, _)| !blocked_set.contains(i))
                    .map(|(i, c)| (i, c.clone()))
                    .collect();

                // Insert the policy-blocked synthetic errors first.
                for (i, reason) in &blocked {
                    assembled[*i] = Some(crate::toolkit::ToolResult {
                        name: calls[*i].name.clone(),
                        result: Err(crate::toolkit::error::ToolError::Execution(format!(
                            "Blocked by safety policy: {}",
                            reason
                        ))),
                    });
                }

                if !permitted.is_empty() {
                    // Run permitted calls in bounded batches, preserving order.
                    for batch in permitted.chunks(MAX_TOOL_CALLS_PER_ROUND) {
                        // RAII: held for the duration of each batch.
                        let _permit = match self.tool_concurrency.acquire().await {
                            Ok(p) => p,
                            Err(e) => {
                                tracing::error!("Tool concurrency semaphore closed: {}", e);
                                stop_reason = StopReason::Cancelled;
                                // Give unexecuted calls a Cancelled result so the
                                // loop below doesn't panic on missing slots.
                                for (i, _) in batch {
                                    assembled[*i] = Some(crate::toolkit::ToolResult {
                                        name: calls[*i].name.clone(),
                                        result: Err(crate::toolkit::error::ToolError::Canceled),
                                    });
                                }
                                break;
                            }
                        };
                        let batch_calls: Vec<crate::toolkit::registry::ToolCall> =
                            batch.iter().map(|(_, c)| c.clone()).collect();
                        let batch_results = registry.execute_parallel(batch_calls).await;
                        for ((idx, _), res) in batch.iter().zip(batch_results) {
                            assembled[*idx] = Some(res);
                        }
                    }
                    // The semaphore being closed means shutdown — do NOT keep
                    // burning LLM rounds; exit the whole round loop.
                    if stop_reason == StopReason::Cancelled {
                        break;
                    }
                }

                // Any slot still None (shouldn't happen) gets a placeholder.
                assembled
                    .into_iter()
                    .enumerate()
                    .map(|(i, opt)| {
                        opt.unwrap_or_else(|| crate::toolkit::ToolResult {
                            name: calls[i].name.clone(),
                            result: Err(crate::toolkit::error::ToolError::Execution(
                                "No result".to_string(),
                            )),
                        })
                    })
                    .collect()
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
                    &original_to_sanitized,
                    &agent.id,
                    execution_id,
                    step_num,
                    &mut large_data_cache,
                    context_window,
                )
                .await;
            step_num = new_step_num;

            // --- Messages compaction ---
            // When the message history grows too large, compact old tool results into
            // short summaries to prevent context window overflow in subsequent rounds.
            compact::compact_executor_messages(messages, context_window);

            // --- Remaining-round countdown ---
            // Small models otherwise run to the budget and get force-summarized
            // by Phase 2; telling them how much runway is left lets them wrap
            // up themselves (synthesize an answer) instead of starting a new
            // tool chain that the cap will cut off. Fires within the last 3
            // rounds — early enough to affect behavior, late enough to not be
            // a daily nag.
            let remaining = max_rounds.saturating_sub(round + 1);
            if remaining > 0 && remaining <= 3 {
                messages.push(Message::new(
                    MessageRole::System,
                    Content::text(format!(
                        "[Budget] {remaining} tool round(s) remain in this run. Report actual outcomes and any unfinished work when the budget ends."
                    )),
                ));
            }

            // The round budget is a hard cap. When the LLM is still tool-calling
            // at the boundary, the post-loop Phase 2 summary synthesizes a final
            // answer from accumulated results — instead of the old `max_rounds
            // += 10` extension hack that masked the real cap and burned extra
            // LLM calls on an already-stuck agent.
            round += 1;
        }

        // If all rounds exhausted without LLM producing final text, OR if LLM failed
        // mid-loop (error message in final_text), use Focused's Phase 2 pattern to
        // generate a natural language conclusion. Never on Cancelled — that's a
        // shutdown signal; synthesizing would fire one more LLM call we're asked
        // to avoid.
        let needs_summary = stop_reason != StopReason::Cancelled
            && (final_text.is_empty()
                || final_text == "LLM generation failed during tool execution."
                || final_text == "Completed tool execution rounds."
                || final_text.trim().is_empty());
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

        if final_text.is_empty() || final_text.trim().is_empty() {
            final_text = "Completed tool execution rounds.".to_string();
        }

        ToolLoopOutput {
            final_text,
            stop_reason,
            all_tool_results,
            round_data_list_raw: round_data_list
                .into_iter()
                .map(|rd| (rd.thought, rd.tool_calls))
                .collect(),
            last_llm_error,
            tokens_used,
        }
    }
}

// ---------------------------------------------------------------------------
// Free functions (no &self)
// ---------------------------------------------------------------------------

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
