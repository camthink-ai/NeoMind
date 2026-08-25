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
    compact, summarize_tool_output, truncate_to, DedupOutcome, RoundData, StopReason,
    ToolCallRecord, ToolLoopOutput,
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
        max_rounds: usize,
        tool_name_map: &std::collections::HashMap<String, String>,
        bound_image: Option<&str>,
    ) -> ToolLoopOutput {
        use crate::agent::tool_parser::parse_tool_calls;
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

        // Get context window for token-aware compaction
        let context_window = llm_runtime.max_context_length();

        let mut round: usize = 0;

        loop {
            if round >= max_rounds {
                tracing::info!(
                    agent_id = %agent.id,
                    max_rounds,
                    "Reached round budget — breaking to Phase 2 summary"
                );
                stop_reason = StopReason::MaxRounds;
                break;
            }

            // Inject accumulated skill reference into system prompt once, after first tool round
            if round > 0 && !skill_reference.is_empty() && !skill_injected {
                if let Some(sys_msg) = messages.first_mut() {
                    sys_msg.content = Content::text(format!(
                        "{}\n\n## Skill Reference\nCommands in this skill are canonical — use them exactly; don't guess subcommand names.\n\n{}",
                        sys_msg.content.as_text(),
                        skill_reference
                    ));
                }
                skill_injected = true;
            }

            let input = LlmInput {
                messages: messages.clone(),
                params: GenerationParams {
                    // AgentDefaults is the /api/settings/agent surface — the
                    // loop used to hardcode 0.7 and ignore it (config only
                    // fed the chat path).
                    temperature: Some(neomind_storage::AgentDefaults::get().default_temperature),
                    top_p: Some(neomind_storage::AgentDefaults::get().default_top_p),
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

            // --- Intra-round + Cross-round deduplication ---
            let dedup_outcome = deduplicate_tool_calls(
                &mut tool_calls,
                &mut all_executed_signatures,
                &agent.id,
                round,
            );

            if matches!(dedup_outcome, DedupOutcome::AllDuplicate) {
                // If we already have tool results to reason over, stop looping —
                // the model's tool vocabulary for this turn is exhausted and the
                // nudged "do something different" call only burns an LLM round
                // (the calls keep dedup-filtering back to empty). Break to the
                // Phase 2 summary so the agent synthesizes from what it has.
                // When we have NO results yet, give the model one more chance.
                if !all_tool_results.is_empty() {
                    self.send_thinking(
                        &agent.id,
                        execution_id,
                        step_num,
                        "All tool calls were duplicates — synthesizing from results so far",
                    )
                    .await;
                    stop_reason = StopReason::AllDuplicate;
                    break;
                }
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

            // Stuck-pattern detection is not a separate mechanism here: the
            // cross-round dedup above IS the loop brake (a fully-duplicated
            // round trips AllDuplicate below and exits via the Phase 2 summary).

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

            // Record SUCCESSFUL executions into the cross-round dedup set.
            // (Failed ones stay out so the model can retry them — the dedup
            // pass only consults the set, never pre-populates it.)
            for (tc, result) in tool_calls.iter().zip(results.iter()) {
                if matches!(&result.result, Ok(o) if o.success) {
                    all_executed_signatures.insert(tool_signature(tc));
                }
            }

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
                    context_window,
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
                        "[System] You have {remaining} tool round(s) left in this run. \
                         If your goal is complete or blocked, give your final answer NOW instead of starting new tool calls."
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
                || final_text == "Completed tool execution rounds.");
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
            stop_reason,
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
            // NOTE: signatures are inserted only AFTER a successful execution
            // (see the caller's record-successful pass). A failed call is NOT
            // deduplicated — the model must be able to retry it (a transient
            // MQTT/extension timeout used to be swallowed as a "duplicate",
            // ending the loop via AllDuplicate with the error in hand).
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
