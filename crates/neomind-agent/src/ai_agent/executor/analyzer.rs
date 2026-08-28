use super::*;
use base64::Engine;

/// Result of situation analysis, branching by execution path.
///
/// - `Free`: Tool-calling mode — the LLM autonomously called tools and
///   produced a full `DecisionProcess` + `ExecutionResult`.  The caller should
///   return these directly, skipping Focused post-processing (execute_decisions,
///   report generation, truncation).
/// - `Focused`: Standard single-pass LLM or rule-based analysis — returns the four
///   classical fields that `execute_internal` assembles into `DecisionProcess`.
pub(crate) enum AnalysisResult {
    Focused {
        situation_analysis: String,
        reasoning_steps: Vec<ReasoningStep>,
        decisions: Vec<Decision>,
        conclusion: String,
    },
    // Boxed: DecisionProcess + ExecutionResult dwarf the Focused fields, and
    // every Focused construction paid their size on the stack (clippy:
    // large_enum_variant).
    Free {
        decision_process: Box<DecisionProcess>,
        execution_result: Box<neomind_storage::ExecutionResult>,
    },
}

impl AgentExecutor {
    pub(crate) async fn analyze_situation_with_intent(
        &self,
        agent: &AiAgent,
        data: &[DataCollected],
        parsed_intent: Option<&neomind_storage::ParsedIntent>,
        execution_id: &str,
        invocation_input: Option<&super::super::AgentInput>,
    ) -> AgentResult<AnalysisResult> {
        tracing::info!(
            agent_id = %agent.id,
            agent_name = %agent.name,
            data_count = data.len(),
            execution_id = %execution_id,
            "[ANALYZE] Starting situation analysis"
        );

        match self.get_llm_runtime_for_agent(agent).await {
            Ok(Some(llm)) => {
                tracing::info!(
                    agent_id = %agent.id,
                    "LLM runtime available, performing LLM-based analysis"
                );

                // Check if tool/function-calling mode should be used
                if self.should_use_tools(agent, &llm) {
                    tracing::info!(
                        agent_id = %agent.id,
                        "Tool mode enabled - using function calling"
                    );
                    return self
                        .execute_with_tools(
                            agent,
                            data,
                            llm.clone(),
                            execution_id,
                            invocation_input,
                        )
                        .await
                        .map(|(dp, exec_result)| {
                            tracing::info!(
                                agent_id = %agent.id,
                                "Tool-based analysis completed successfully"
                            );
                            AnalysisResult::Free {
                                decision_process: Box::new(dp),
                                execution_result: Box::new(exec_result),
                            }
                        })
                        .map_err(|e| {
                            tracing::warn!(
                                agent_id = %agent.id,
                                error = %e,
                                "Tool-based analysis failed"
                            );
                            e
                        });
                }

                // Standard LLM-based analysis
                return self
                    .analyze_with_llm(llm, agent, data, parsed_intent, execution_id)
                    .await
                    .map(|(sa, rs, ds, con)| {
                        tracing::info!(
                            agent_id = %agent.id,
                            "LLM-based analysis completed successfully"
                        );
                        AnalysisResult::Focused {
                            situation_analysis: sa,
                            reasoning_steps: rs,
                            decisions: ds,
                            conclusion: con,
                        }
                    })
                    .map_err(|e| {
                        tracing::warn!(
                            agent_id = %agent.id,
                            error = %e,
                            "LLM analysis failed"
                        );
                        e
                    });
            }
            Ok(None) => {
                if agent.llm_backend_id.is_some() {
                    // Agent expected an LLM but runtime is unavailable.
                    // Fail explicitly rather than silently degrading.
                    tracing::warn!(
                        agent_id = %agent.id,
                        backend_id = ?agent.llm_backend_id,
                        "Agent has llm_backend_id but runtime is unavailable"
                    );
                    return Err(NeoMindError::Validation(format!(
                        "Agent '{}' 配置了 LLM 后端但运行时不可用（可能已被删除或 API key 未配置）",
                        agent.name
                    )));
                }
                tracing::info!(
                    agent_id = %agent.id,
                    "Rule-only agent (no llm_backend_id), proceeding to rule-based analysis"
                );
                // Fall through to rule-based analysis below
            }
            Err(e) => {
                tracing::warn!(
                    agent_id = %agent.id,
                    error = %e,
                    "Failed to load LLM runtime"
                );
                return Err(e);
            }
        }

        // Only reached for rule-only agents (no llm_backend_id).
        let (situation_analysis, reasoning_steps, decisions, conclusion) =
            self.analyze_rule_based(agent, data, parsed_intent).await?;
        Ok(AnalysisResult::Focused {
            situation_analysis,
            reasoning_steps,
            decisions,
            conclusion,
        })
    }

    /// Legacy analysis path for LLMs that don't support function calling.
    ///
    /// Simplified: uses `build_tool_system_prompt` (without tools) and requests
    /// plain-text analysis instead of fragile JSON. Derives ReasoningStep/Decision
    /// from the text output deterministically.
    pub(crate) async fn analyze_with_llm(
        &self,
        llm: Arc<dyn neomind_core::llm::backend::LlmRuntime + Send + Sync>,
        agent: &AiAgent,
        data: &[DataCollected],
        _parsed_intent: Option<&neomind_storage::ParsedIntent>,
        execution_id: &str,
    ) -> AgentResult<(String, Vec<ReasoningStep>, Vec<Decision>, String)> {
        use neomind_core::llm::backend::{GenerationParams, LlmInput};

        tracing::info!(
            agent_id = %agent.id,
            data_count = data.len(),
            execution_id,
            "Legacy Focused path: calling LLM for plain-text analysis"
        );

        // Collect image parts from data
        let (image_parts, image_sources_info) = collect_image_parts(data);
        let llm_supports_vision = llm.capabilities().supports_images;
        let has_valid_images = !image_parts.is_empty() && llm_supports_vision;

        // Log vision decision
        tracing::info!(
            target: "neomind::agent::event_value",
            agent_id = %agent.id,
            model_name = %llm.model_name(),
            llm_supports_vision,
            image_parts_count = image_parts.len(),
            has_valid_images,
            "[DIAG] legacy analyzer vision decision"
        );
        if !image_parts.is_empty() && !llm_supports_vision {
            // Event-agent running blind: images are present but the configured
            // LLM can't see them. Elevated to error so monitoring catches a
            // misconfigured camera-event agent — the chat path rejects this
            // outright (session.rs: "select a vision-capable model"), but the
            // event-agent path degrades. The analysis still runs text-only to
            // avoid breaking existing flows; the prompt below surfaces the
            // limitation into the output.
            tracing::error!(
                agent_id = %agent.id,
                model_name = %llm.model_name(),
                image_count = image_parts.len(),
                "Vision misconfiguration: camera event agent received images but the active \
                 LLM does not support vision — analyzing text-only. Configure a vision-capable \
                 backend (or set a multimodal override) for this agent to actually see images."
            );
        }

        // Build system prompt using the shared builder (no tools, no invocation)
        let config = ToolLoopConfig::focused_plus(agent);
        let knowledge_content = self.prefetch_knowledge_files(
            &agent.id,
            &agent.memory.knowledge_files,
            llm.max_context_length(),
        );
        let system_prompt = super::tool_prompt::build_tool_system_prompt(
            agent,
            data,
            None,
            &config,
            knowledge_content.as_ref(),
        );

        // Build user message with data summary
        let data_lines = build_compact_data_summary(data);
        let image_info = if !image_sources_info.is_empty() && !has_valid_images {
            format!(
                "\n\n[Image data unavailable — LLM does not support vision: {}]",
                image_sources_info.join(", ")
            )
        } else {
            String::new()
        };

        // User message instructing plain-text output. When running blind
        // (images present but the model can't see them), ask the LLM to
        // surface that limitation in its findings so a reader of the output
        // knows the images weren't analyzed — instead of a confident
        // text-only conclusion. No forced prepend, so downstream consumers
        // parsing the findings aren't broken.
        let blind = !image_info.is_empty();
        let user_msg_text = if blind {
            format!(
                "{}{}\n\nAnalyze the data above and provide your findings as plain text. \
                 Note: the image capture(s) could not be analyzed because the configured \
                 model lacks vision — mention this limitation in your findings, then proceed \
                 with text-only analysis. Include your conclusion at the end.",
                data_lines.join("\n"),
                image_info,
            )
        } else {
            format!(
                "{}{}\n\nAnalyze the data above and provide your findings as plain text. \
                 Include your conclusion at the end.",
                data_lines.join("\n"),
                image_info,
            )
        };

        // Build multimodal messages if images present
        let messages = if has_valid_images {
            let mut parts = vec![ContentPart::text(user_msg_text)];
            for (_source, _data_type, image_content) in &image_parts {
                match image_content {
                    ImageContent::Base64(data, mime) => {
                        parts.push(ContentPart::image_base64(data.clone(), mime.clone()));
                    }
                    ImageContent::Url(url) => {
                        parts.push(ContentPart::image_url(url.clone()));
                    }
                }
            }
            vec![
                Message::new(MessageRole::System, Content::text(system_prompt)),
                Message::from_parts(MessageRole::User, parts),
            ]
        } else {
            vec![
                Message::new(MessageRole::System, Content::text(system_prompt)),
                Message::new(MessageRole::User, Content::text(user_msg_text)),
            ]
        };

        let input = LlmInput {
            messages,
            params: GenerationParams {
                temperature: Some(0.7),
                max_tokens: Some(8000),
                thinking_enabled: Some(false),
                ..Default::default()
            },
            model: None,
            stream: false,
            tools: None,
        };

        // LLM call with timeout
        const LLM_TIMEOUT_SECS: u64 = 300;
        let llm_result = match tokio::time::timeout(
            std::time::Duration::from_secs(LLM_TIMEOUT_SECS),
            llm.generate(input),
        )
        .await
        {
            Ok(result) => result,
            Err(_) => {
                tracing::warn!(agent_id = %agent.id, "LLM timed out after {}s", LLM_TIMEOUT_SECS);
                return Err(NeoMindError::Llm(format!(
                    "LLM timeout after {}s",
                    LLM_TIMEOUT_SECS
                )));
            }
        };

        let output = llm_result.map_err(|e| {
            tracing::warn!(
                agent_id = %agent.id,
                error = %e,
                permanent = e.is_permanent(),
                "LLM generation failed"
            );
            NeoMindError::Llm(format!("{}", e))
        })?;

        let text = output.text.trim().to_string();
        if text.is_empty() {
            tracing::warn!(agent_id = %agent.id, "LLM returned empty response");
            // Build a meaningful fallback from the data we already collected,
            // rather than returning a generic "No analysis produced."
            let data_summary = if data_lines.is_empty() {
                "No data was available for analysis.".to_string()
            } else {
                let summary: Vec<&str> = data_lines.iter().map(|s| s.as_str()).take(10).collect();
                format!(
                    "Collected data (LLM analysis unavailable):\n{}",
                    summary.join("\n")
                )
            };
            return Ok((
                data_summary.clone(),
                vec![ReasoningStep {
                    step_number: 1,
                    description: "LLM returned empty response — falling back to raw data summary"
                        .to_string(),
                    step_type: "fallback".to_string(),
                    input: None,
                    output: String::new(),
                    confidence: 0.3,
                }],
                vec![Decision {
                    decision_type: "info".to_string(),
                    description: "LLM analysis unavailable, returning raw data".to_string(),
                    action: "log".to_string(),
                    rationale: "LLM returned no content".to_string(),
                    expected_outcome: "Raw data preserved for user review".to_string(),
                }],
                data_summary,
            ));
        }

        tracing::info!(
            agent_id = %agent.id,
            text_len = text.len(),
            "Legacy Focused analysis completed"
        );

        // Derive structured output from plain text
        let situation_analysis = truncate_to(&text, 2000);
        let char_count = text.chars().count();
        let conclusion = if char_count > 500 {
            let start = char_count - 500;
            text.chars().skip(start).collect::<String>()
        } else {
            text.clone()
        };
        let reasoning_steps = vec![ReasoningStep {
            step_number: 1,
            description: truncate_to(&text, 400),
            step_type: "llm_analysis".to_string(),
            input: Some(format!("{} data sources", data.len())),
            output: truncate_to(&text, 500),
            confidence: 0.7,
        }];
        let decisions = vec![Decision {
            decision_type: "info".to_string(),
            description: truncate_to(&text, 200),
            action: "log".to_string(),
            rationale: "Legacy path analysis completed".to_string(),
            expected_outcome: conclusion.clone(),
        }];

        // Emit events
        if let Some(ref bus) = self.event_bus {
            let ts = chrono::Utc::now().timestamp();
            for step in &reasoning_steps {
                let _ = bus
                    .publish(NeoMindEvent::AgentThinking {
                        agent_id: agent.id.clone(),
                        execution_id: execution_id.to_string(),
                        step_number: step.step_number,
                        step_type: step.step_type.clone(),
                        description: step.description.clone(),
                        details: None,
                        timestamp: ts,
                    })
                    .await;
            }
            for decision in &decisions {
                let _ = bus
                    .publish(NeoMindEvent::AgentDecision {
                        agent_id: agent.id.clone(),
                        execution_id: execution_id.to_string(),
                        description: decision.description.clone(),
                        rationale: decision.rationale.clone(),
                        action: decision.action.clone(),
                        confidence: 0.7,
                        timestamp: ts,
                    })
                    .await;
            }
        }

        Ok((situation_analysis, reasoning_steps, decisions, conclusion))
    }

    pub(crate) async fn analyze_rule_based(
        &self,
        agent: &AiAgent,
        data: &[DataCollected],
        parsed_intent: Option<&neomind_storage::ParsedIntent>,
    ) -> AgentResult<(String, Vec<ReasoningStep>, Vec<Decision>, String)> {
        let mut reasoning_steps = Vec::new();
        let mut decisions = Vec::new();

        // Step 1: Understand the situation
        let situation_analysis = if data.is_empty() {
            format!(
                "No pre-collected data for agent '{}'. Analyzing based on user instructions and configured rules.",
                agent.name
            )
        } else {
            format!(
                "Analyzing {} data points for agent '{}'",
                data.len(),
                agent.name
            )
        };

        reasoning_steps.push(ReasoningStep {
            step_number: 1,
            description: "Collect and analyze input data".to_string(),
            step_type: "data_collection".to_string(),
            input: Some(format!("{} data sources", data.len())),
            output: format!("Data collected from {} sources", data.len()),
            confidence: 1.0,
        });

        // Step 2: Evaluate conditions based on parsed intent
        let intent = parsed_intent.or(agent.parsed_intent.as_ref());
        if let Some(intent) = intent {
            for condition in &intent.conditions {
                let result = self.evaluate_condition(condition, data).await;

                reasoning_steps.push(ReasoningStep {
                    step_number: reasoning_steps.len() as u32 + 1,
                    description: format!("Evaluate condition: {}", condition),
                    step_type: "condition_eval".to_string(),
                    input: Some(condition.clone()),
                    output: format!("Condition result: {}", result),
                    confidence: 0.8,
                });

                if result {
                    decisions.push(Decision {
                        decision_type: "condition_met".to_string(),
                        description: format!("Condition '{}' is met", condition),
                        action: "trigger_actions".to_string(),
                        rationale: format!("The condition '{}' evaluated to true", condition),
                        expected_outcome: "Execute defined actions".to_string(),
                    });
                }
            }
        }

        // Step 3: Determine actions
        let empty_actions = vec![];
        let actions = intent.map(|i| &i.actions).unwrap_or(&empty_actions);
        if !decisions.is_empty() {
            for action in actions {
                reasoning_steps.push(ReasoningStep {
                    step_number: reasoning_steps.len() as u32 + 1,
                    description: format!("Plan action: {}", action),
                    step_type: "action_planning".to_string(),
                    input: Some(action.clone()),
                    output: format!("Action '{}' queued for execution", action),
                    confidence: 0.7,
                });
            }
        }

        let conclusion = if decisions.is_empty() {
            if data.is_empty() {
                "No data available and no conditions met. Consider binding data sources or using an LLM-powered agent for flexible analysis.".to_string()
            } else {
                "No actions required - conditions not met".to_string()
            }
        } else {
            format!("{} action(s) to be executed", decisions.len())
        };

        Ok((situation_analysis, reasoning_steps, decisions, conclusion))
    }

    pub(crate) async fn evaluate_condition(&self, condition: &str, data: &[DataCollected]) -> bool {
        let condition_lower = condition.to_lowercase();

        // Check if any data meets the condition
        for data_item in data {
            if let Some(value) = data_item.values.get("value") {
                if let Some(num) = value.as_f64() {
                    if condition_lower.contains("大于")
                        || condition_lower.contains(">")
                        || condition_lower.contains("超过")
                    {
                        if let Some(threshold) = extract_threshold(&condition_lower) {
                            return num > threshold;
                        }
                    } else if condition_lower.contains("小于")
                        || condition_lower.contains("<")
                        || condition_lower.contains("低于")
                    {
                        if let Some(threshold) = extract_threshold(&condition_lower) {
                            return num < threshold;
                        }
                    }
                }
            }
        }

        false
    }
}

// ---------------------------------------------------------------------------
// Helper functions extracted from analyze_with_llm
// ---------------------------------------------------------------------------

/// Collect image parts from `DataCollected` entries.
///
/// Returns `(image_parts, image_sources_info)`:
/// - `image_parts`: validated image content (URL or cleaned base64) with source/type metadata.
/// - `image_sources_info`: human-readable description strings for the text summary.
fn collect_image_parts(
    data: &[DataCollected],
) -> (Vec<(String, String, ImageContent)>, Vec<String>) {
    let mut image_parts = Vec::new();
    let mut image_sources_info = Vec::new(); // Track image sources for text summary

    for d in data.iter() {
        let is_image = d
            .values
            .get("_is_image")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        if !is_image {
            continue;
        }

        // Record image source info for text summary
        image_sources_info.push(format!(
            "[图像数据: {}, 格式: {}]",
            d.source,
            d.values
                .get("image_mime_type")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown")
        ));

        // Try to get image URL first
        if let Some(url) = d.values.get("image_url").and_then(|v| v.as_str()) {
            if !url.is_empty() {
                image_parts.push((
                    d.source.clone(),
                    d.data_type.clone(),
                    ImageContent::Url(url.to_string()),
                ));
                continue;
            }
        }

        // Fall back to base64 data
        if let Some(base64) = d.values.get("image_base64").and_then(|v| v.as_str()) {
            if !base64.is_empty() {
                // Prefer stored mime → fall back to magic-prefix
                // inference → final jpeg fallback.
                let mime = d
                    .values
                    .get("image_mime_type")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())
                    .or_else(|| {
                        crate::image_utils::infer_mime_from_base64_prefix(base64)
                            .map(|s| s.to_string())
                    })
                    .unwrap_or_else(|| "image/jpeg".to_string());
                // Clean base64: strip whitespace/newlines, remove non-base64 characters
                let cleaned_base64: String = base64
                    .chars()
                    .filter(|c| c.is_ascii_alphanumeric() || *c == '+' || *c == '/' || *c == '=')
                    .collect();
                // Fix padding
                let padded_len = (cleaned_base64.len() + 3) & !3;
                let padded_base64 = if cleaned_base64.len() < padded_len {
                    let mut s = cleaned_base64;
                    for _ in 0..(padded_len - s.len()) {
                        s.push('=');
                    }
                    s
                } else {
                    cleaned_base64
                };
                // Try standard decoding first, then URL-safe
                let decoded = base64::engine::general_purpose::STANDARD
                    .decode(&padded_base64)
                    .or_else(|_| {
                        // Try URL-safe base64 (uses - and _ instead of + and /)
                        let url_safe_fixed: String =
                            padded_base64.replace('-', "+").replace('_', "/");
                        base64::engine::general_purpose::STANDARD.decode(&url_safe_fixed)
                    });
                match decoded {
                    Ok(bytes) => {
                        tracing::debug!(
                            source = %d.source,
                            size_kb = bytes.len() / 1024,
                            "Validated base64 image data"
                        );
                        // Re-encode as clean standard base64 for Ollama
                        let clean = base64::engine::general_purpose::STANDARD.encode(&bytes);
                        image_parts.push((
                            d.source.clone(),
                            d.data_type.clone(),
                            ImageContent::Base64(clean, mime.to_string()),
                        ));
                    }
                    Err(e) => {
                        tracing::warn!(
                            source = %d.source,
                            len = base64.len(),
                            error = %e,
                            "Skipping invalid base64 image data"
                        );
                        continue;
                    }
                }
            }
        }
    }

    (image_parts, image_sources_info)
}

/// Build a compact text summary of non-image data for the LLM prompt.
///
/// Filters out images, memory-internal data types, and placeholder entries,
/// then formats up to 15 metrics as `- source: type = value` lines.
fn build_compact_data_summary(data: &[DataCollected]) -> Vec<String> {
    let max_metrics = 15;
    data.iter()
        .filter(|d| {
            // Exclude images
            if d.values
                .get("_is_image")
                .and_then(|v| v.as_bool())
                .unwrap_or(false)
            {
                return false;
            }
            // Exclude memory-related data types (confuses small models)
            let data_type_lower = d.data_type.to_lowercase();
            if matches!(
                data_type_lower.as_str(),
                "summary" | "memory" | "state_variables" | "baselines" | "patterns"
            ) {
                return false;
            }
            // Exclude placeholder data from collect_data
            // When no data sources are bound, collect_data adds a placeholder with guidance
            // This placeholder should NOT be treated as real sensor data
            if d.source == "system"
                && d.values
                    .get("message")
                    .and_then(|v| v.as_str())
                    .map(|s| s.contains("No pre-collected data"))
                    .unwrap_or(false)
            {
                return false;
            }
            true
        })
        .take(max_metrics)
        .map(|d| {
            // Create a more compact representation of values
            let value_str = if let Some(v) = d.values.get("value") {
                format!("{}", v) // Compact value representation
            } else if let Some(v) = d.values.get("history") {
                format!(
                    "[历史数据: {}个点]",
                    v.as_array().map(|a| a.len()).unwrap_or(0)
                )
            } else {
                // Fallback to compact JSON - use character-safe truncation
                let json_str = serde_json::to_string(&d.values).unwrap_or_default();
                if json_str.chars().count() > 200 {
                    // Truncate at character boundary, not byte boundary
                    json_str.chars().take(200).collect::<String>() + "..."
                } else {
                    json_str
                }
            };
            format!("- {}: {} = {}", d.source, d.data_type, value_str)
        })
        .collect()
}
