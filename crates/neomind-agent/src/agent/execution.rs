//! `execution` — split from the former agent/mod.rs monolith.

use super::*;

use crate::error::NeoMindError;

use serde_json::Value;

use crate::error::Result;

use crate::tools::mapper::map_tool_parameters;

use neomind_core::Message;

impl Agent {
    pub(crate) async fn process_with_llm(&self, user_message: &str) -> Result<AgentResponse> {
        tracing::debug!(message = %user_message, "process_with_llm starting");
        use tool_parser::parse_tool_calls;

        // Get existing history (user message already added by caller in `process`)
        // Optimize: Clone only needed messages in one pass, avoiding double-clone
        let mut history_without_last: Vec<AgentMessage> = {
            let state = self.internal_state.read().await;
            let memory = &state.memory;
            if memory.len() > 1 {
                // Clone only what we need (skip last message)
                memory.iter().take(memory.len() - 1).cloned().collect()
            } else {
                Vec::new()
            }
        };

        // === CHAT HISTORY DEPTH (configurable, /api/settings/agent) ===
        apply_chat_history_depth(&mut history_without_last);

        // === DYNAMIC CONTEXT WINDOW: Get model's actual capacity ===
        // Query the LLM backend for the actual context window size.
        // Reserve space for: system prompt, user message, and generation
        let max_context = self.llm_interface.max_context_length().await;

        // Calculate space needed for non-history components
        // System prompt (~500 tokens) + user message (~200 tokens) + context injection (~200 tokens) + generation reserve (~1000 tokens)
        const NON_HISTORY_TOKENS: usize = 1900;
        let safe_max_context = max_context.saturating_sub(NON_HISTORY_TOKENS);

        // === P3.2: ADAPTIVE CONTEXT SIZING ===
        // Adjust context size based on conversation complexity:
        // - High entity diversity: +10%
        // - Multiple active topics: +10%
        // - Recent errors: +15%
        // - Simple greetings: -10%
        let adaptive_adjustment = calculate_adaptive_context_adjustment(&history_without_last);

        // Calculate effective max with adaptive adjustment, ensuring we don't exceed safe limit
        let effective_max = ((safe_max_context as f64) * adaptive_adjustment) as usize;

        // Enforce reasonable bounds (minimum 1024 tokens for history, maximum safe limit)
        let effective_max = effective_max.clamp(1024, safe_max_context);

        tracing::debug!(
            "Context window: model_capacity={}, safe_max={}, adjustment={:.2}, effective_max={}",
            max_context,
            safe_max_context,
            adaptive_adjustment,
            effective_max
        );

        // === BUDGET-FIRST SHORT-CIRCUIT ===
        // Compaction is LOSSY (old user messages truncated to 200 chars,
        // assistant turns squeezed to one-line summaries). Running it before
        // knowing the budget destroyed history even when the window had ample
        // room. Measure first: when the whole (depth-capped) history fits the
        // effective window, pass it through untouched — the lossy pipeline
        // below only runs when it genuinely doesn't fit.
        let history_tokens: usize = history_without_last
            .iter()
            .map(tokenizer::estimate_message_tokens)
            .sum();
        let compacted_history = if history_tokens <= effective_max {
            tracing::debug!(
                history_tokens,
                effective_max,
                msgs = history_without_last.len(),
                "History fits budget — skipping lossy compaction"
            );
            history_without_last.clone()
        } else {
            // === ANTHROPIC-STYLE IMPROVEMENT: Apply context window with tool result clearing ===
            // This prevents context bloat from old tool calls while maintaining conversation continuity
            // Uses compaction cache for incremental updates when only a few messages changed
            let compacted_history = {
                let mut state = self.internal_state.write().await;
                let current_count = state.memory.len().saturating_sub(1); // without last

                // Check cache validity: same max_tokens and small message delta
                let cached = state.compaction_cache.take();
                let compacted = if let Some((cached_count, cached_max, ref cached_msgs)) = cached {
                    if cached_max == effective_max
                        && current_count > cached_count
                        && current_count <= cached_count + 4
                    {
                        // Incremental: only compact the new messages and append
                        let new_msgs: Vec<AgentMessage> = history_without_last
                            .iter()
                            .skip(cached_count)
                            .cloned()
                            .collect();
                        let mut base = cached_msgs.clone();
                        if !new_msgs.is_empty() {
                            // Re-compact the tail with existing context
                            // For small deltas, just append (tool compaction will handle on next full run)
                            base.extend(new_msgs);
                            build_context_window(&base, effective_max)
                        } else {
                            base
                        }
                    } else {
                        // Full recompaction needed
                        build_context_window(&history_without_last, effective_max)
                    }
                } else {
                    build_context_window(&history_without_last, effective_max)
                };

                // Update cache
                state.compaction_cache = Some((current_count, effective_max, compacted.clone()));
                compacted
            };

            compacted_history
        };

        tracing::debug!(
            "Context: {} messages -> {} messages (after compaction)",
            history_without_last.len(),
            compacted_history.len()
        );

        // Build history for LLM (convert AgentMessage to Message)
        let mut core_history: Vec<Message> =
            compacted_history.iter().map(|msg| msg.to_core()).collect();

        // === CONVERSATION CONTEXT: Inject context summary ONLY if it changed ===
        // This prevents repeatedly injecting the same context which can cause
        // the LLM to generate repetitive responses
        let context_summary = {
            let shared = self.shared_state.read().await;
            let summary = shared.conversation_context.get_context_summary();
            if !summary.is_empty() {
                Some(format!("Current conversation context:\n{}", summary))
            } else {
                None
            }
        };

        // Only inject context if it has changed since last time
        // Use a simple hash to detect changes
        if let Some(summary) = context_summary {
            use std::collections::hash_map::DefaultHasher;
            use std::hash::{Hash, Hasher};

            let summary_hash = {
                let mut h = DefaultHasher::new();
                summary.hash(&mut h);
                h.finish()
            };

            let mut shared = self.shared_state.write().await;
            if shared.last_injected_context_hash != summary_hash {
                // Context has changed, inject it
                shared.last_injected_context_hash = summary_hash;
                drop(shared); // Release lock before proceeding

                use neomind_core::message::{Content, MessageRole};
                core_history.push(Message::new(MessageRole::System, Content::text(&summary)));
                tracing::debug!(
                    "Injected conversation context into LLM history (changed from previous)"
                );
            } else {
                drop(shared); // Release lock
                tracing::debug!("Skipping context injection - unchanged from previous");
            }
        }

        // Call LLM with conversation history (user message will be added by LLM interface)
        let chat_response = self
            .llm_interface
            .chat_with_history(user_message, &core_history)
            .await
            .map_err(|e| crate::error::NeoMindError::Llm(e.to_string()))?;

        // Parse response for tool calls
        tracing::debug!(response_text = %chat_response.text, "LLM response received");
        let (content, mut tool_calls) = parse_tool_calls(&chat_response.text)?;
        tracing::debug!(count = tool_calls.len(), "Parsed tool calls");
        for tc in &tool_calls {
            tracing::debug!(name = %tc.name, args = %tc.arguments, "  tool call");
        }

        // Extract thinking content if present
        let thinking = chat_response.thinking;

        // If no tool calls in response content, try parsing from thinking field
        // Some models (like qwen3 with thinking enabled) may put tool calls in thinking
        if tool_calls.is_empty() {
            if let Some(ref thinking_content) = thinking {
                if let Ok((_, thinking_tool_calls)) = parse_tool_calls(thinking_content) {
                    if !thinking_tool_calls.is_empty() {
                        tracing::debug!("Found tool calls in thinking field, using them");
                        tool_calls = thinking_tool_calls;
                    }
                }
            }
        }

        // If no tool calls, return the direct response
        if tool_calls.is_empty() {
            // Save assistant response with or without thinking
            let assistant_msg = if let Some(thinking_content) = thinking {
                // Apply cleanup to thinking if it's too long
                let cleaned_thinking = if thinking_content.len() > 200 {
                    crate::agent::streaming::cleanup_thinking_content(&thinking_content)
                } else {
                    thinking_content
                };
                AgentMessage::assistant_with_thinking(&content, &cleaned_thinking)
            } else {
                AgentMessage::assistant(&content)
            };

            // === SAFEGUARD: Register response for cross-turn repetition detection ===
            {
                let mut state = self.internal_state.write().await;
                state.register_response(&content);
                state.push_message(assistant_msg.clone());
            }

            return Ok(AgentResponse {
                message: assistant_msg,
                tool_calls: vec![],
                memory_context_used: true,
                tools_used: vec![],
                processing_time_ms: 0,
            });
        }

        // === SAFEGUARD: Limit number of tool calls to prevent infinite loops ===
        let max_calls = self.config.max_tool_calls;
        if tool_calls.len() > max_calls {
            tracing::warn!(
                "Too many tool calls ({}) in single request, limiting to {}",
                tool_calls.len(),
                max_calls
            );
            tool_calls.truncate(max_calls);
        }

        // === DEDUPLICATE: Remove duplicate tool calls to avoid redundant execution ===
        // Models sometimes output the same tool call multiple times
        // We keep the first occurrence of each unique (name, arguments) pair
        let original_count = tool_calls.len();
        let mut seen = std::collections::HashSet::new();
        tool_calls.retain(|tool_call| {
            // Create a unique key based on tool name and arguments
            let key = (
                tool_call.name.clone(),
                tool_call
                    .arguments
                    .to_string()
                    .chars()
                    .take(100)
                    .collect::<String>(),
            );
            seen.insert(key)
        });
        let dedup_count = tool_calls.len();
        if original_count > dedup_count {
            tracing::debug!(
                "Deduplicated tool calls: {} -> {} (removed {} duplicates)",
                original_count,
                dedup_count,
                original_count - dedup_count
            );
        }

        // Tool calls detected - DON'T save the initial assistant message yet
        // We'll save a complete message (with tool_calls and final response) after tool execution

        // === DEPENDENCY-AWARE TOOL SCHEDULING ===
        // Group tools into execution batches based on dependencies:
        // - Tools in the same batch can execute in parallel
        // - Tools in later batches wait for results from earlier batches
        // - Mutually exclusive tools are detected and handled
        let execution_batches = self.build_execution_batches(&tool_calls).await;

        tracing::debug!(
            count = execution_batches.len(),
            batches = ?execution_batches.iter().map(|b| b.len()).collect::<Vec<_>>(),
            "Tool execution batches (dependency-aware)"
        );

        let mut tool_results = Vec::new();
        let mut tools_used = Vec::new();
        let mut tool_calls_with_results = Vec::new();

        // Execute each batch in sequence
        for (batch_idx, batch) in execution_batches.iter().enumerate() {
            if batch.len() > 1 {
                tracing::debug!(
                    batch = batch_idx,
                    size = batch.len(),
                    "Executing batch in parallel"
                );
            }

            // Clone tool_calls for parallel execution within this batch
            let batch_clone: Vec<_> = batch.to_vec();
            let semaphore = self.tool_concurrency_limit.clone();

            // Use futures for parallel execution within batch (limited by semaphore)
            let futures: Vec<_> = batch_clone
                .into_iter()
                .map(|tool_call| {
                    let name = tool_call.name.clone();
                    let arguments = tool_call.arguments.clone();
                    let id = tool_call.id.clone();
                    let sem = semaphore.clone();

                    async move {
                        let _permit = match sem.acquire().await {
                            Ok(p) => p,
                            Err(e) => {
                                tracing::warn!("tool concurrency semaphore closed: {}", e);
                                return (
                                    name,
                                    id,
                                    arguments,
                                    Err(NeoMindError::Tool(
                                        "tool concurrency semaphore closed".to_string(),
                                    )),
                                );
                            }
                        };
                        let result = self.execute_tool(&name, &arguments).await;
                        (name, id, arguments, result)
                    }
                })
                .collect();

            // Execute all tools in this batch in parallel and wait for completion
            let results = futures::future::join_all(futures).await;

            // Process results in original order
            for (name, id, arguments, result) in results {
                tracing::debug!(name = %name, result = ?result, "Tool execution result");
                // Push the resolved tool name (e.g., "rule" instead of "list_rules")
                let resolved = self.resolve_tool_name(&name);
                tools_used.push(resolved);
                tracing::debug!(name = %name, count = tools_used.len(), "Added to tools_used");
                match result {
                    Ok(ok_result) => {
                        tool_results.push((name.clone(), ok_result.clone()));
                        tool_calls_with_results.push(ToolCall {
                            name,
                            id,
                            arguments,
                            result: Some(serde_json::json!(ok_result)),
                            round: None,
                        });
                    }
                    Err(e) => {
                        let error_msg = format!("Error: {}", e);
                        tool_results.push((name.clone(), error_msg.clone()));
                        tool_calls_with_results.push(ToolCall {
                            name,
                            id,
                            arguments,
                            result: Some(serde_json::json!({ "error": error_msg })),
                            round: None,
                        });
                    }
                }
            }
        }

        tracing::debug!(tools_used = ?tools_used, "Formatting tool results");

        // Format tool results directly (unified ReAct loop — no Phase 2)
        let final_text = crate::agent::streaming::format_tool_results(&tool_results);

        // Save a complete message with tool_calls, results, and optionally thinking
        let final_message = if let Some(thinking_content) = thinking {
            // Clean up thinking if it's too long
            let cleaned_thinking = if thinking_content.len() > 200 {
                crate::agent::streaming::cleanup_thinking_content(&thinking_content)
            } else {
                thinking_content
            };
            AgentMessage::assistant_with_tools_and_thinking(
                &final_text,
                tool_calls_with_results.clone(),
                &cleaned_thinking,
            )
        } else {
            AgentMessage::assistant_with_tools(&final_text, tool_calls_with_results.clone())
        };
        self.internal_state
            .write()
            .await
            .push_message(final_message.clone());

        Ok(AgentResponse {
            message: final_message,
            // Accumulated across every round of the multi-round loop — the
            // first round alone under-reports turns where the model
            // investigates before acting (and made eval metrics blind to
            // exactly that behavior).
            tool_calls: tool_calls_with_results,
            memory_context_used: true,
            tools_used,
            processing_time_ms: 0,
        })
    }
}
impl Agent {
    /// Map simplified parameter names to actual tool parameter names.
    ///
    /// This bridges the gap between the user-friendly simplified interface
    /// and the actual tool implementation parameters.
    pub(crate) fn map_simplified_parameters(&self, tool_name: &str, arguments: &Value) -> Value {
        if let Some(args_obj) = arguments.as_object() {
            // Special handling for create_rule: convert simplified (name, condition, action) to DSL
            if tool_name == "create_rule" || tool_name == "rule_from_context" {
                // Check if we have simplified parameters (condition + action) but no dsl
                let has_condition = args_obj.contains_key("condition");
                let has_action = args_obj.contains_key("action");
                let has_description = args_obj.contains_key("description");
                let has_dsl = args_obj.contains_key("dsl");

                if (has_condition || has_description) && !has_dsl {
                    // Convert simplified parameters to DSL
                    let name = args_obj
                        .get("name")
                        .and_then(|v| v.as_str())
                        .unwrap_or("未命名规则");

                    let dsl = if has_description {
                        // rule_from_context: extract structured rule from description
                        let description = args_obj
                            .get("description")
                            .and_then(|v| v.as_str())
                            .unwrap_or("");

                        // Try to parse the description to extract condition/action
                        // For now, generate a simple DSL with the description as context
                        format!(
                            r#"RULE "{name}"
WHEN sensor.temperature > 30
DO
  NOTIFY "{description}"
END"#
                        )
                    } else if has_condition && has_action {
                        // create_rule with simplified condition/action
                        let condition = args_obj
                            .get("condition")
                            .and_then(|v| v.as_str())
                            .unwrap_or("sensor.temperature > 30");

                        let action = args_obj
                            .get("action")
                            .and_then(|v| v.as_str())
                            .unwrap_or("通知管理员");

                        format!(
                            r#"RULE "{name}"
WHEN {condition}
DO
  NOTIFY "{action}"
END"#
                        )
                    } else {
                        // Fallback: just use the name
                        format!(
                            r#"RULE "{name}"
WHEN sensor.temperature > 30
DO
  NOTIFY "规则触发"
END"#
                        )
                    };

                    let mut mapped = serde_json::Map::new();
                    mapped.insert("name".to_string(), serde_json::json!(name));
                    mapped.insert("dsl".to_string(), serde_json::json!(dsl));

                    // Include description if present
                    if let Some(desc) = args_obj.get("description") {
                        mapped.insert("description".to_string(), desc.clone());
                    }

                    return serde_json::Value::Object(mapped);
                }
            }

            // Delegate to mapper.rs for standard parameter mapping
            map_tool_parameters(tool_name, arguments)
        } else {
            arguments.clone()
        }
    }
}
impl Agent {
    /// Map simplified tool names to real tool names.
    ///
    /// Simplified names are used in LLM prompts (e.g., "device_discover")
    /// Real names are the same as simplified names since we unified to underscore naming.
    ///
    /// This now uses the unified `ToolNameMapper` to ensure consistency
    /// across the codebase.
    pub(crate) fn resolve_tool_name(&self, simplified_name: &str) -> String {
        // Delegate to the unified mapper
        crate::tools::resolve_tool_name(simplified_name)
    }
}
impl Agent {
    /// Build execution batches based on tool dependencies.
    ///
    /// Returns a Vec of batches where:
    /// - Each batch contains tools that can be executed in parallel
    /// - Later batches depend on results from earlier batches
    /// - Tools with dependencies are scheduled after their prerequisites
    ///
    /// Uses ToolRelationships metadata from tool definitions:
    /// - `call_after`: Prerequisites that must execute first
    /// - `output_to`: Tools that depend on this tool's output
    /// - `exclusive_with`: Tools that cannot run together
    async fn build_execution_batches(&self, tool_calls: &[ToolCall]) -> Vec<Vec<ToolCall>> {
        use std::collections::{HashMap, HashSet};

        if tool_calls.is_empty() {
            return vec![];
        }

        // If only one tool, no batching needed
        if tool_calls.len() == 1 {
            return vec![tool_calls.to_vec()];
        }

        // Build maps for normalized tool names and relationships
        // We need to track both original names (for ToolCall cloning) and resolved names (for dependency resolution)
        let mut original_to_resolved: HashMap<String, String> = HashMap::new();
        let mut resolved_to_original: HashMap<String, String> = HashMap::new();

        // First pass: resolve all tool names
        for tool_call in tool_calls {
            let real_name = self.resolve_tool_name(&tool_call.name);
            original_to_resolved.insert(tool_call.name.clone(), real_name.clone());
            resolved_to_original
                .entry(real_name)
                .or_insert_with(|| tool_call.name.clone());
        }

        // Build relationships map using RESOLVED names as keys
        let mut relationships: HashMap<String, neomind_core::tools::ToolRelationships> =
            HashMap::new();
        for tool_call in tool_calls {
            let real_name = self.resolve_tool_name(&tool_call.name);
            if let Some(tool) = self.tools.get(&real_name) {
                relationships.insert(real_name, tool.definition().relationships);
            } else {
                // No relationships found, use default
                relationships.insert(real_name, neomind_core::tools::ToolRelationships::default());
            }
        }

        // Create a resolved name to ToolCall map
        let mut resolved_to_call: HashMap<String, ToolCall> = HashMap::new();
        for tool_call in tool_calls {
            let real_name = self.resolve_tool_name(&tool_call.name);
            resolved_to_call.insert(real_name, tool_call.clone());
        }

        // Track which tools are in each batch (using resolved names)
        let mut batches: Vec<Vec<ToolCall>> = vec![];
        let mut placed_resolved: HashSet<String> = HashSet::new();

        // Kahn's algorithm for topological sorting
        loop {
            let mut current_batch = Vec::new();
            let mut current_batch_resolved: HashSet<String> = HashSet::new();

            for tool_call in tool_calls {
                let resolved_name = &original_to_resolved[&tool_call.name];

                // Skip if already placed
                if placed_resolved.contains(resolved_name) {
                    continue;
                }

                // Get dependencies (call_after)
                let deps = relationships
                    .get(resolved_name)
                    .map(|r| r.call_after.clone())
                    .unwrap_or_default();

                // Check if all dependencies are satisfied
                // Dependencies are specified using resolved tool names
                let deps_satisfied = deps.iter().all(|dep| {
                    // Resolve the dependency name to handle any aliases in the dependency spec
                    let resolved_dep = self.resolve_tool_name(dep);
                    // Dependency is satisfied if:
                    // 1. It's not in our current tool_calls (not being executed)
                    // 2. Or it's already been placed in a previous batch
                    !resolved_to_call.contains_key(&resolved_dep)
                        || placed_resolved.contains(&resolved_dep)
                });

                if !deps_satisfied {
                    continue; // Wait for dependencies
                }

                // Check for mutual exclusivity with tools in current batch
                let exclusive_with = relationships
                    .get(resolved_name)
                    .map(|r| r.exclusive_with.clone())
                    .unwrap_or_default();

                let conflicts_with_batch = exclusive_with.iter().any(|excl| {
                    let resolved_excl = self.resolve_tool_name(excl);
                    current_batch_resolved.contains(&resolved_excl)
                });

                if conflicts_with_batch {
                    continue; // Can't run with current batch, try next batch
                }

                // Can add to current batch
                current_batch.push(tool_call.clone());
                current_batch_resolved.insert(resolved_name.clone());
            }

            if current_batch.is_empty() {
                // No more tools can be placed
                break;
            }

            // Mark tools as placed
            for name in current_batch_resolved {
                placed_resolved.insert(name);
            }

            batches.push(current_batch);

            // Exit if all tools are placed
            if placed_resolved.len() == tool_calls.len() {
                break;
            }
        }

        // Handle circular dependency case (tools still not placed)
        if placed_resolved.len() < tool_calls.len() {
            tracing::warn!(
                placed = placed_resolved.len(),
                total = tool_calls.len(),
                "Circular dependency detected, remaining tools will execute in single batch"
            );
            // Add remaining tools as a final batch
            let remaining: Vec<_> = tool_calls
                .iter()
                .filter(|tc| !placed_resolved.contains(&original_to_resolved[&tc.name]))
                .cloned()
                .collect();
            if !remaining.is_empty() {
                batches.push(remaining);
            }
        }

        // If batching didn't work (shouldn't happen), fall back to single batch
        if batches.is_empty() {
            batches.push(tool_calls.to_vec());
        }

        batches
    }
}
