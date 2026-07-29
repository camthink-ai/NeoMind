//! Tool result processing and final result construction.
//!
//! Handles processing tool results (building messages, truncation, skill handling)
//! and constructing the final DecisionProcess and ExecutionResult from tool loop output.

use std::collections::HashMap;
use std::sync::Arc;

use neomind_core::llm::backend::LlmRuntime;
use neomind_core::message::{Content, Message, MessageRole};
use neomind_storage::{AiAgent, DataCollected, Decision, DecisionProcess, ReasoningStep};

use super::{
    resolve_role, summarize_tool_output, truncate_to, ToolLoopOutput, TOOL_RESULT_MAX_LEN,
};

use super::super::AgentExecutor;

/// Targeted guidance when the LLM hallucinates a tool name that doesn't exist.
///
/// LLMs trained on many agent frameworks often invent a `message`/`notify`/`alert`
/// tool, or try to call a neomind CLI *domain* (`device`, `dashboard`, ...) as if
/// it were a standalone tool. Rather than returning a bare "not found", redirect
/// them to the real mechanism (`shell` → `neomind <domain> ...`) so they
/// self-correct next round. Returns None for names with no specific hint
/// (caller falls back to listing the actually-available tools).
pub(crate) fn hallucinated_tool_hint(tool_name: &str) -> Option<String> {
    let lower = tool_name.to_lowercase();
    // 1. Message/alert family — give the exact send syntax (most common hallucination).
    if matches!(
        lower.as_str(),
        "message"
            | "notify"
            | "notification"
            | "alert"
            | "send_message"
            | "send_notification"
            | "send_alert"
    ) {
        return Some(format!(
            " There is NO `{}` tool. To send notifications or alerts, use the `shell` tool with: \
             `neomind message send --title \"<title>\" --body \"<body>\" --severity \
             <info|warning|error|critical>`.",
            tool_name
        ));
    }
    // 2. neomind CLI domains used as tool names — redirect to shell.
    //    (Reuses the canonical domain list from the mapper to stay in sync.)
    if crate::tools::mapper::CLI_DOMAINS.contains(&lower.as_str()) {
        return Some(format!(
            " There is NO `{}` tool — `{}` is a neomind CLI domain. Use the `shell` tool: \
             `neomind {} <action>` (e.g. `neomind {} list`, or `neomind {} --help` for all actions).",
            tool_name, tool_name, lower, lower, lower
        ));
    }
    // 3. A full `neomind <...>` command emitted as a tool name. Small/weak models
    //    do this when they can't map intent -> shell(command=...): they emit the
    //    whole command (e.g. "neomind device list") as the tool name with empty
    //    args, get NotFound, and loop. Redirect verbatim so they recover in one
    //    round instead of looping.
    if lower.trim_start().starts_with("neomind ") {
        let cmd = tool_name.trim();
        return Some(format!(
            " There is NO `{}` tool — that is a shell command. Re-issue it via the `shell` tool \
             with arguments {{\"command\": \"{cmd}\"}}.",
            tool_name
        ));
    }
    None
}

impl AgentExecutor {
    /// List the names of tools actually registered, for inclusion in NotFound hints.
    ///
    /// This is the universal safety net ("保底"): when the LLM hallucinates any tool
    /// name that has no targeted redirect, it still learns what *does* exist —
    /// including dynamically-registered extension tools.
    fn available_tool_names(&self) -> String {
        if let Some(reg) = self.tool_registry.read().as_ref() {
            let mut names: Vec<String> = reg.list();
            names.sort();
            if !names.is_empty() {
                return names.join(", ");
            }
        }
        // Static fallback if the registry isn't initialized yet.
        "shell, memory, skill".to_string()
    }

    /// Process tool results: append to all_tool_results, build messages, handle skill results.
    ///
    /// Returns the updated step number after emitting thinking events.
    pub(crate) async fn process_tool_results(
        &self,
        results: &[crate::toolkit::ToolResult],
        messages: &mut Vec<Message>,
        all_tool_results: &mut Vec<crate::toolkit::ToolResult>,
        skill_reference: &mut String,
        original_to_sanitized: &HashMap<String, String>,
        agent_id: &str,
        execution_id: &str,
        mut step_num: u32,
        cache: &mut crate::agent::types::LargeDataCache,
    ) -> u32 {
        for result in results {
            all_tool_results.push(result.clone());
            let result_text = match &result.result {
                Ok(output) => {
                    let raw = serde_json::to_string(&output.data)
                        .unwrap_or_else(|_| "Success".to_string());
                    // Slim large/base64 strings into the cache BEFORE sanitize.
                    // Sanitize alone strips `data:image/` to a useless `[image data, N B]`
                    // marker — slim replaces them with `$cached:` references the LLM can
                    // pass back to image-aware tools (vision/image_edit). Must run first
                    // so sanitize sees plain text (no `data:image/` prefix) and skips its
                    // own stripping path.
                    let slimmed = {
                        match serde_json::from_str::<serde_json::Value>(&raw) {
                            Ok(mut v) => {
                                let n = cache.slim_large_strings_in_json(&mut v, &result.name);
                                if n > 0 {
                                    tracing::debug!(
                                        tool = %result.name,
                                        slimmed_values = n,
                                        "Slimmed large strings from scheduled-agent tool result"
                                    );
                                    serde_json::to_string(&v).unwrap_or_else(|_| raw.clone())
                                } else {
                                    raw
                                }
                            }
                            Err(_) => raw,
                        }
                    };
                    // Sanitize base64/image data to prevent context bloat
                    let sanitized =
                        crate::agent::streaming::sanitize_tool_result_for_prompt(&slimmed);
                    // UTF-8 safe truncation (has fast-path for short strings)
                    // 128KB limit: large enough for compact time-series and multi-device
                    // queries. The compaction layer handles context window limits later.
                    crate::agent::streaming::truncate_result_utf8(&sanitized, TOOL_RESULT_MAX_LEN)
                }
                Err(e) => {
                    let err_msg = format!("Error: {}", e);
                    // Add actionable hint so the LLM can adjust its strategy.
                    // Match on the NotFound *variant* (not a substring) so that an
                    // Execution error whose message happens to contain "not found"
                    // (e.g. "Device 'abc' not found") doesn't falsely trigger the
                    // hallucinated-tool redirect.
                    let hint = if matches!(e, crate::toolkit::ToolError::NotFound(_)) {
                        hallucinated_tool_hint(&result.name).unwrap_or_else(|| {
                            let available = self.available_tool_names();
                            format!(
                                " Tool '{}' does not exist. Available tools: {}. \
                                 Use `shell` for any neomind CLI command.",
                                result.name, available
                            )
                        })
                    } else if e.to_string().contains("Invalid arguments")
                        || e.to_string().contains("missing")
                    {
                        " Check parameter names and types.".to_string()
                    } else if e.to_string().contains("timed out") {
                        " The operation took too long. Try a simpler query.".to_string()
                    } else if e.to_string().contains("Permission denied") {
                        " This action is not allowed. Try an alternative approach.".to_string()
                    } else {
                        String::new()
                    };
                    format!("{}{}", err_msg, hint)
                }
            };

            // Skill tool results go to separate reference buffer, not messages history
            if result.name == "skill" {
                if !skill_reference.is_empty() {
                    skill_reference.push_str("\n\n");
                }
                skill_reference.push_str(&result_text);
                // Add a concise acknowledgment to messages so LLM knows the skill was retrieved
                messages.push(Message::new(
                    MessageRole::User,
                    Content::text("Skill guide retrieved and will be used as reference."),
                ));
            } else {
                // Use sanitized name for LLM message so it matches what the LLM used
                let msg_name = original_to_sanitized
                    .get(&result.name)
                    .cloned()
                    .unwrap_or_else(|| result.name.clone());
                messages.push(Message::tool_result(&msg_name, &result_text));
            }

            // Send thinking event for each tool result
            let result_preview = match &result.result {
                Ok(output) => {
                    let brief = summarize_tool_output(&output.data, &result.name);
                    truncate_to(&brief, 200).to_string()
                }
                Err(e) => format!("Error: {}", e),
            };
            self.send_thinking(
                agent_id,
                execution_id,
                step_num,
                &format!("tool '{}' → {}", result.name, result_preview),
            )
            .await;
            step_num += 1;
        }
        step_num
    }

    /// Generate a Phase 2 summary when the tool loop exhausted rounds without final text.
    ///
    /// Uses the Focused Phase 2 pattern: sends full tool results in natural language
    /// format so the LLM can produce a real analysis, NOT a JSON template.
    pub(crate) async fn generate_phase2_summary(
        &self,
        agent: &AiAgent,
        llm_runtime: &Arc<dyn LlmRuntime + Send + Sync>,
        all_tool_results: &[crate::toolkit::ToolResult],
        round_count: usize,
    ) -> Option<String> {
        use neomind_core::llm::backend::{GenerationParams, LlmInput};

        // Build follow-up prompt — natural language, NOT JSON template.
        // Includes full tool results so the LLM can produce a real analysis.
        let task = &agent.user_prompt;
        let mut phase2_user = format!(
            "{}\n\n[Completed {} rounds of tool execution, {} tool results collected]\n\
             IMPORTANT: You MUST analyze ALL tool results below and provide a COMPLETE response. \
             Do NOT just say \"execution completed\" — present the data naturally.\n\n",
            task,
            round_count.max(1),
            all_tool_results.len(),
        );

        for r in all_tool_results {
            let result_text = match &r.result {
                Ok(output) => {
                    let raw = serde_json::to_string(&output.data)
                        .unwrap_or_else(|_| "Success".to_string());
                    // Sanitize base64/image data to prevent context bloat
                    let sanitized = crate::agent::streaming::sanitize_tool_result_for_prompt(&raw);
                    crate::agent::streaming::truncate_result_utf8(&sanitized, TOOL_RESULT_MAX_LEN)
                }
                Err(e) => format!("Error: {}", e),
            };
            phase2_user.push_str(&format!("[{}]\n{}\n\n", r.name, result_text));
        }
        phase2_user.push_str(&format!(
            "\nPlease organize the above data to answer: {}",
            task
        ));

        // Phase 2 summary: include agent role for domain-aware analysis.
        // The agent's custom system_prompt provides domain context (e.g. "temperature
        // monitoring agent") that produces more relevant summaries than a generic role.
        let default_role = format!("You are an intelligent IoT agent named '{}'.", agent.name);
        let agent_role = resolve_role(agent, &default_role);
        let summary_role = format!(
            "{}\n\nAnalyze the tool execution results and provide a comprehensive, \
             user-friendly response in the SAME language as the task. \
             Focus on the actual data and insights, not on mentioning that tools were called.",
            agent_role
        );
        let summary_messages = vec![
            Message::new(MessageRole::System, Content::text(summary_role)),
            Message::new(MessageRole::User, Content::text(&phase2_user)),
        ];

        let summary_input = LlmInput {
            messages: summary_messages,
            params: GenerationParams {
                temperature: Some(0.7),
                max_tokens: Some(8192),
                // Disable thinking for Phase 2 fallback summary (gotcha #7):
                // this summarizes already-collected tool results, not user-facing
                // reasoning. Thinking models would burn tokens on hidden CoT
                // before producing the summary.
                thinking_enabled: Some(false),
                ..Default::default()
            },
            model: None,
            stream: false,
            tools: None, // No tools — force LLM to answer, not call more tools
        };

        match llm_runtime.generate(summary_input).await {
            Ok(output) => {
                let text = output.text.trim().to_string();
                let response_len = text.len();
                if !text.is_empty() {
                    tracing::debug!(
                        agent_id = %agent.id,
                        response_len,
                        "Phase 2 analysis generated successfully"
                    );
                    Some(text)
                } else {
                    None
                }
            }
            Err(e) => {
                tracing::warn!(
                    agent_id = %agent.id,
                    error = %e,
                    "Failed to generate Phase 2 analysis"
                );
                // Return None — build_tool_result will generate fallback
                None
            }
        }
    }
}

/// Map tool name to semantic decision type for memory pattern extraction.
pub(crate) fn tool_name_to_semantic_type(tool_name: &str) -> &'static str {
    match tool_name {
        // Notification tools → alert
        "message" | "send_message" | "send_notification" => "alert",
        // Shell can execute anything — classify as command
        "shell" => "command",
        // Raw function names
        "execute_command" | "execute_extension_command" | "control_device" => "command",
        "query_metric" | "get_latest_metrics" | "list_devices" | "get_device_info" => "info",
        // Extension tools (format: "extension-id_command")
        name if name.contains('_') => {
            if name.contains("control") || name.contains("execute") || name.contains("set") {
                "command"
            } else if name.contains("notify") || name.contains("alert") || name.contains("send") {
                "alert"
            } else {
                "info"
            }
        }
        // Default: info (read-only operations)
        _ => "info",
    }
}

/// Build the final DecisionProcess and ExecutionResult from tool loop output.
pub(crate) fn build_tool_result(
    agent: &AiAgent,
    data_collected: &[DataCollected],
    loop_output: ToolLoopOutput,
) -> (DecisionProcess, neomind_storage::ExecutionResult) {
    let ToolLoopOutput {
        final_text,
        stop_reason,
        all_tool_results,
        round_data_list_raw,
        last_llm_error: _,
    } = loop_output;

    tracing::info!(
        agent_id = %agent.id,
        stop_reason = stop_reason.label(),
        "Tool loop ended"
    );

    // === Free mode: LLM natural language response is the primary output ===
    // Tool calls already executed all actions. The final_text is the LLM's
    // summary/analysis for the user — use it directly as conclusion.

    let llm_failed = final_text == "LLM generation failed during tool execution.";

    // --- situation_analysis: use the LLM's first-round thinking as context summary ---
    let situation_analysis = round_data_list_raw
        .iter()
        .find_map(|(thought, _)| thought.as_ref().filter(|t| !t.is_empty()))
        .cloned()
        .unwrap_or_else(|| {
            if llm_failed {
                "LLM model API call failed.".to_string()
            } else if all_tool_results.is_empty() {
                "No tools were executed.".to_string()
            } else {
                format!("Executed {} tool operation(s).", all_tool_results.len())
            }
        });

    // --- conclusion: LLM's natural language response, directly ---
    let is_generic =
        final_text.is_empty() || final_text == "Completed tool execution rounds." || llm_failed;

    let conclusion = if !is_generic {
        final_text.clone()
    } else if llm_failed {
        "LLM model call failed. Please check that the bound model is available and supports the required capabilities (e.g., multimodal for image analysis).".to_string()
    } else if !all_tool_results.is_empty() {
        let tool_summary: Vec<String> = all_tool_results
            .iter()
            .map(|r| match &r.result {
                Ok(output) => summarize_tool_output(&output.data, &r.name),
                Err(e) => format!("{} failed: {}", r.name, e),
            })
            .collect();
        tool_summary.join("; ") + "."
    } else {
        "No tools were executed during this agent run.".to_string()
    };

    // --- reasoning steps ---
    let mut reasoning_steps: Vec<ReasoningStep> = Vec::new();
    let mut step_counter = 0u32;

    for (thought, tool_calls) in &round_data_list_raw {
        if let Some(thought) = thought {
            step_counter += 1;
            reasoning_steps.push(ReasoningStep {
                step_number: step_counter,
                description: thought.clone(),
                step_type: "thought".to_string(),
                input: None,
                output: String::new(),
                confidence: 0.8,
            });
        }

        for tc in tool_calls {
            step_counter += 1;
            let (desc, conf, step_type) = match &tc.result.result {
                Ok(output) => (
                    format!("Executed tool '{}'", tc.name),
                    if output.success { 0.9 } else { 0.3 },
                    "tool_call",
                ),
                Err(e) => (format!("Tool '{}' failed: {}", tc.name, e), 0.2, "error"),
            };

            let input_str = serde_json::to_string(&tc.input).ok();
            let output_str = match &tc.result.result {
                Ok(output) => serde_json::to_string(&output.data).unwrap_or_default(),
                Err(e) => format!("Error: {}", e),
            };

            reasoning_steps.push(ReasoningStep {
                step_number: step_counter,
                description: desc,
                step_type: step_type.to_string(),
                input: input_str,
                output: output_str,
                confidence: conf,
            });
        }
    }

    let decisions: Vec<Decision> = all_tool_results
        .iter()
        .map(|r| {
            let (desc, action) = match &r.result {
                Ok(output) => {
                    let action_summary = summarize_tool_output(&output.data, &r.name);
                    (format!("Executed tool '{}'", r.name), action_summary)
                }
                Err(e) => (format!("Tool '{}' failed", r.name), format!("Error: {}", e)),
            };
            let semantic_type = tool_name_to_semantic_type(&r.name);
            Decision {
                decision_type: semantic_type.to_string(),
                description: desc,
                action,
                rationale: format!("Tool '{}' executed successfully", r.name),
                expected_outcome: String::new(),
            }
        })
        .collect();

    // Confidence: based on tool success rate
    let final_confidence = if all_tool_results.is_empty() {
        0.5
    } else {
        let ok = all_tool_results.iter().filter(|r| r.result.is_ok()).count() as f32;
        (ok / all_tool_results.len() as f32).max(0.5)
    };

    let decision_process = DecisionProcess {
        situation_analysis,
        data_collected: data_collected.to_vec(),
        reasoning_steps,
        decisions,
        conclusion,
        confidence: final_confidence,
        stop_reason: stop_reason.label().to_string(),
    };

    let actions_executed: Vec<neomind_storage::ActionExecuted> = all_tool_results
        .iter()
        .map(|r| {
            let success = r.result.is_ok();
            neomind_storage::ActionExecuted {
                action_type: "tool_call".to_string(),
                description: format!("Execute tool '{}'", r.name),
                target: r.name.clone(),
                parameters: serde_json::Value::Null,
                success,
                result: if success {
                    r.result.as_ref().ok().map(|o| o.data.to_string())
                } else {
                    r.result.as_ref().err().map(|e| e.to_string())
                },
            }
        })
        .collect();

    let success_rate = if actions_executed.is_empty() {
        1.0
    } else {
        actions_executed.iter().filter(|a| a.success).count() as f32 / actions_executed.len() as f32
    };

    // summary: the actual LLM response text.
    // Skip generic/error strings — the frontend already shows conclusion separately.
    let summary_text = if final_text.is_empty()
        || final_text == "Completed tool execution rounds."
        || final_text == "LLM generation failed during tool execution."
    {
        String::new()
    } else {
        final_text.clone()
    };
    let execution_result = neomind_storage::ExecutionResult {
        actions_executed,
        report: None,
        notifications_sent: vec![],
        summary: summary_text,
        success_rate,
    };

    tracing::debug!(
        agent_id = %agent.id,
        tool_calls = all_tool_results.len(),
        success_rate,
        "Tool execution completed"
    );

    (decision_process, execution_result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hallucinated_tool_hint_redirects_message_aliases() {
        // Common hallucinated names should all redirect to shell.
        for name in &[
            "message",
            "notify",
            "notification",
            "alert",
            "send_message",
            "send_notification",
            "send_alert",
            "MESSAGE", // case-insensitive
        ] {
            let hint = hallucinated_tool_hint(name);
            assert!(hint.is_some(), "{:?} should be redirected", name);
            assert!(
                hint.unwrap().contains("neomind message send"),
                "hint must show the correct shell command"
            );
        }
    }

    #[test]
    fn test_hallucinated_tool_hint_unknown_name_is_none() {
        // Genuinely unknown tool names fall back to the generic hint (None here).
        assert!(hallucinated_tool_hint("device_list").is_none());
        assert!(hallucinated_tool_hint("shell").is_none());
        assert!(hallucinated_tool_hint("").is_none());
    }

    #[test]
    fn test_hallucinated_tool_hint_redirects_cli_domains() {
        // CLI domains used as tool names redirect to the shell tool.
        for name in &["device", "dashboard", "rule", "agent", "widget", "system"] {
            let hint = hallucinated_tool_hint(name);
            assert!(
                hint.is_some(),
                "{:?} should be redirected as a CLI domain",
                name
            );
            let h = hint.unwrap();
            assert!(h.contains("shell"), "must point to shell: {}", h);
            assert!(
                h.contains("neomind"),
                "must reference the neomind CLI: {}",
                h
            );
        }
    }

    #[test]
    fn test_hallucinated_tool_hint_redirects_full_neomind_command() {
        // Weak models emit a whole `neomind ...` command as the tool name (e.g.
        // "neomind device list") with empty args, then loop on NotFound. Redirect
        // verbatim to shell so they recover next round.
        for name in &[
            "neomind device list",
            "neomind settings set-timezone Asia/Shanghai",
            "neomind rule create",
            "  neomind system info  ",
        ] {
            let hint = hallucinated_tool_hint(name);
            assert!(hint.is_some(), "{:?} should redirect as a full neomind command", name);
            let h = hint.unwrap();
            assert!(h.contains("shell"), "must point to shell: {}", h);
            let trimmed = name.trim();
            assert!(h.contains(trimmed), "must echo the verbatim command {:?}: {}", trimmed, h);
        }
        // Real tool names and unrelated strings still get no hint.
        assert!(hallucinated_tool_hint("shell").is_none());
        assert!(hallucinated_tool_hint("skill").is_none());
        assert!(hallucinated_tool_hint("random_thing").is_none());
    }
}
