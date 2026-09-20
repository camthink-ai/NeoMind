//! `process` — split from the former agent/mod.rs monolith.

use super::*;

use std::pin::Pin;

use futures::Stream;

use crate::error::NeoMindError;

use crate::error::Result;

impl Agent {
    /// === FAST PATH: Check for simple responses BEFORE acquiring lock ===
    /// This improves latency for common queries like greetings and confirmations.
    fn try_fast_path(&self, user_message: &str) -> Option<AgentResponse> {
        let trimmed = user_message.trim().to_lowercase();
        let start = std::time::Instant::now();

        // Greeting patterns
        let greeting_responses: &[(&str, &str)] = &[
            ("你好", "你好！我是 NeoMind 智能助手，有什么可以帮您？"),
            ("您好", "您好！我是 NeoMind 智能助手，有什么可以帮您？"),
            (
                "hi",
                "Hello! I'm NeoMind, your smart assistant. How can I help you?",
            ),
            ("hello", "Hello! I'm NeoMind, your smart assistant."),
            ("早上好", "早上好！今天有什么可以帮您的？"),
            ("下午好", "下午好！有什么可以帮您的？"),
            ("晚上好", "晚上好！有什么可以帮您的？"),
        ];

        // Confirmation patterns
        let confirmation_responses: &[(&str, &str)] = &[
            ("好的", "好的，我明白了。"),
            ("好的，", "好的。"),
            ("明白", "好的，我明白了。"),
            ("明白了", "好的，我明白了。"),
            ("知道了", "好的，我知道了。"),
            ("收到", "好的，收到了。"),
            ("嗯", "好的，我明白了。"),
            ("行", "好的，没问题。"),
            ("是", "是的，我明白了。"),
            ("对", "是的，正确。"),
            ("ok", "OK!"),
            ("好的ok", "好的！"),
            ("谢谢", "不客气！还有其他需要帮助的吗？"),
            (
                "thanks",
                "You're welcome! Is there anything else I can help with?",
            ),
        ];

        // Check greetings — EXACT match only. Prefix matching (starts_with)
        // misfired on substantive messages: "ok here's my question..." -> "OK!",
        // "行业发展" -> "好的，没问题." (matched "行"), "对称性" -> "是的，正确."
        // (matched "对"). Anything that isn't exactly the greeting falls through
        // to the LLM, which handles greetings contextually AND preserves intent
        // (e.g. "ok, create a device" no longer short-circuits to a canned "OK!").
        for (pattern, response) in greeting_responses.iter() {
            if trimmed == *pattern {
                return Some(AgentResponse {
                    message: AgentMessage::assistant(*response),
                    tool_calls: vec![],
                    memory_context_used: false,
                    tools_used: vec![],
                    processing_time_ms: start.elapsed().as_millis() as u64,
                });
            }
        }

        // Check confirmations — EXACT match only (same rationale as greetings).
        for (pattern, response) in confirmation_responses.iter() {
            if trimmed == *pattern {
                return Some(AgentResponse {
                    message: AgentMessage::assistant(*response),
                    tool_calls: vec![],
                    memory_context_used: false,
                    tools_used: vec![],
                    processing_time_ms: start.elapsed().as_millis() as u64,
                });
            }
        }

        None
    }
}
impl Agent {
    /// Process a user message with real LLM.
    /// Uses session-level lock to prevent concurrent requests on the same session.
    pub async fn process(&self, user_message: &str) -> Result<AgentResponse> {
        tracing::debug!(message = %user_message, "Agent::process starting");

        // === FAST PATH: Try simple responses WITHOUT acquiring lock ===
        if let Some(response) = self.try_fast_path(user_message) {
            // Save to history for context continuity
            let user_msg = AgentMessage::user(user_message);
            self.internal_state.write().await.push_message(user_msg);
            self.internal_state
                .write()
                .await
                .push_message(response.message.clone());

            return Ok(response);
        }

        // === NORMAL PATH: Acquire lock for complex processing ===
        let _lock = self.process_lock.lock().await;

        // Refresh tool definitions on every turn so mid-session toggles
        // (user disabled a tool via the Extensions page) take effect on the
        // next chat turn instead of requiring a new session. Cheap: filters
        // an in-memory Vec and only re-pushes if the LLM interface changed.
        self.update_tool_definitions().await;

        // Set session ID on memory tool to avoid cross-session contamination
        // (must happen under process_lock so concurrent calls to THIS session serialize)
        self.tools
            .set_memory_session_id(self.session_id.clone())
            .await;

        let start = std::time::Instant::now();

        // === SMART FOLLOWUP INTERCEPTION (Context-Aware) ===
        // Single lock acquisition for both context and followup
        let followup_analysis = {
            let mut shared = self.shared_state.write().await;
            let ctx_snapshot = shared.conversation_context.clone();
            shared
                .smart_followup
                .analyze_input(user_message, &ctx_snapshot)
        };

        // Handle smart followup cases
        if !followup_analysis.can_proceed {
            let response_content = if let Some(first_followup) = followup_analysis.followups.first()
            {
                // Use the highest priority followup
                let mut content = first_followup.question.clone();

                // Add suggestions if available
                if !first_followup.suggestions.is_empty() {
                    content.push_str("\n\nSuggested options:");
                    for (i, suggestion) in first_followup.suggestions.iter().enumerate() {
                        content.push_str(&format!("\n{}. {}", i + 1, suggestion));
                    }
                }

                content
            } else {
                // Should not reach here, but fallback
                "I understand your request, but need more information.".to_string()
            };

            // Save user message and our response to history
            let user_msg = AgentMessage::user(user_message);
            let response_msg = AgentMessage::assistant(&response_content);

            self.internal_state.write().await.push_message(user_msg);
            self.internal_state
                .write()
                .await
                .push_message(response_msg.clone());

            return Ok(AgentResponse {
                message: response_msg,
                tool_calls: vec![],
                memory_context_used: true,
                tools_used: vec![],
                processing_time_ms: start.elapsed().as_millis() as u64,
            });
        }

        // === SMART CONVERSATION INTERCEPTION (Simple, Fallback) ===
        // Simple pattern-based interception for backward compatibility
        let smart_analysis = {
            let smart_conv = self.smart_conversation.read().await;
            smart_conv.analyze_input(user_message)
        };

        // Handle cases where we should intercept
        if !smart_analysis.can_proceed {
            let response_content = if let Some(question) = smart_analysis.missing_info {
                // Information missing - ask user
                format!("❓ {}", question)
            } else if let Some(confirm) = smart_analysis.requires_confirmation {
                // Dangerous operation - require confirmation
                format!("⚠️ {}", confirm)
            } else if let Some(clarify) = smart_analysis.ambiguous {
                // Intent unclear - ask for clarification
                format!("❓ {}", clarify)
            } else {
                // Should not reach here, but fallback
                "我明白您的请求，但需要更多信息。".to_string()
            };

            // Save user message and our response to history
            let user_msg = AgentMessage::user(user_message);
            let response_msg = AgentMessage::assistant(&response_content);

            self.internal_state.write().await.push_message(user_msg);
            self.internal_state
                .write()
                .await
                .push_message(response_msg.clone());

            return Ok(AgentResponse {
                message: response_msg,
                tool_calls: vec![],
                memory_context_used: true,
                tools_used: vec![],
                processing_time_ms: start.elapsed().as_millis() as u64,
            });
        }

        // === CONVERSATION CONTEXT: Enhance input with conversation state ===
        let enhanced_input = {
            let shared = self.shared_state.read().await;
            if let Some(resolved) = shared
                .conversation_context
                .resolve_ambiguous_command(user_message)
            {
                resolved
            } else {
                shared.conversation_context.enhance_input(user_message)
            }
        };

        // Add user message to history (use enhanced version for processing, but save original)
        let user_msg = AgentMessage::user(user_message);
        self.internal_state
            .write()
            .await
            .push_message(user_msg.clone());

        // Check if LLM is configured
        if !self.llm_interface.is_ready().await {
            // Fall back to simple keyword-based responses
            let (message, tool_calls, tools_used) =
                process_fallback(&self.tools, &self.fallback_rules, user_message).await;
            let processing_time = start.elapsed().as_millis() as u64;

            self.internal_state
                .write()
                .await
                .push_message(message.clone());

            return Ok(AgentResponse {
                message,
                tool_calls,
                memory_context_used: true,
                tools_used,
                processing_time_ms: processing_time,
            });
        }

        // === LLM PATH: Process with real LLM ===
        // Note: Fast path responses (greetings, confirmations) are handled in try_fast_path()
        // before acquiring the lock to improve latency.
        match self.process_with_llm(&enhanced_input).await {
            Ok(response) => {
                // === CONVERSATION CONTEXT: Update context after successful response ===
                {
                    let tool_results: Vec<(String, String)> = response
                        .tool_calls
                        .iter()
                        .filter_map(|tc| {
                            tc.result.as_ref().map(|r| {
                                (
                                    tc.name.clone(),
                                    serde_json::to_string(r)
                                        .unwrap_or_else(|_| "无结果".to_string()),
                                )
                            })
                        })
                        .collect();
                    let mut shared = self.shared_state.write().await;
                    shared
                        .conversation_context
                        .update(user_message, &tool_results);
                }

                let processing_time = start.elapsed().as_millis() as u64;
                self.internal_state
                    .write()
                    .await
                    .session
                    .increment_messages();
                Ok(AgentResponse {
                    processing_time_ms: processing_time,
                    ..response
                })
            }
            Err(e) => {
                // On error, fall back to simple response
                tracing::error!(error = %e, "LLM error, using fallback");
                let (message, tool_calls, tools_used) =
                    process_fallback(&self.tools, &self.fallback_rules, user_message).await;
                let processing_time = start.elapsed().as_millis() as u64;

                self.internal_state
                    .write()
                    .await
                    .push_message(message.clone());

                Ok(AgentResponse {
                    message,
                    tool_calls,
                    memory_context_used: true,
                    tools_used,
                    processing_time_ms: processing_time,
                })
            }
        }
    }
}
impl Agent {
    /// Process a user message with images (multimodal input).
    ///
    /// This method is used when the user sends images along with their text message.
    /// The images should be base64-encoded data URLs (e.g., "data:image/png;base64,...").
    pub async fn process_multimodal(
        &self,
        user_message: &str,
        images: Vec<String>, // Base64 data URLs
    ) -> Result<AgentResponse> {
        tracing::debug!(
            message = %user_message,
            image_count = images.len(),
            "Agent::process_multimodal starting"
        );

        // Create multimodal message content AND prepare images for storage
        let mut parts = vec![neomind_core::ContentPart::text(user_message)];
        let mut user_images = Vec::new();

        // Process images for both ContentPart and storage
        for image_data in &images {
            // Parse via shared utility — handles data URLs, raw base64, and
            // jpg/jpeg aliasing consistently across all multimodal paths.
            let parsed = crate::image_utils::parse_image_data(image_data).unwrap_or(
                crate::image_utils::ParsedImage {
                    mime_type: "image/png",
                    base64: image_data.as_str(),
                },
            );
            let mime_type_str = parsed.mime_type;
            let base64_part = parsed.base64;

            // Add to ContentPart for LLM
            parts.push(neomind_core::ContentPart::image_base64(
                base64_part,
                mime_type_str,
            ));

            // Add to storage as AgentMessageImage
            user_images.push(crate::agent::types::AgentMessageImage {
                data: image_data.clone(),
                mime_type: Some(mime_type_str.to_string()),
            });
        }

        let user_msg = neomind_core::Message::new(
            neomind_core::MessageRole::User,
            neomind_core::Content::Parts(parts),
        );

        // === Skip fast path for multimodal messages (always use LLM) ===
        let _lock = self.process_lock.lock().await;
        let start = std::time::Instant::now();

        // Add user message to history WITH images (for multimodal context in follow-up requests)
        let agent_user_msg = AgentMessage::user_with_images(user_message, user_images);
        self.internal_state
            .write()
            .await
            .push_message(agent_user_msg);

        // Check if LLM is configured (required for multimodal)
        if !self.llm_interface.is_ready().await {
            return Err(NeoMindError::Llm(
                "Multimodal input requires LLM support".to_string(),
            ));
        }

        // === Get conversation history ===
        // Optimize: Clone only needed messages in one pass
        let history_without_last: Vec<AgentMessage> = {
            let state = self.internal_state.read().await;
            let memory = &state.memory;
            if memory.len() > 1 {
                memory.iter().take(memory.len() - 1).cloned().collect()
            } else {
                Vec::new()
            }
        };

        // Convert AgentMessage history to Message history
        let core_history: Vec<neomind_core::Message> = history_without_last
            .iter()
            .map(|msg| msg.to_core())
            .collect();

        // === Process with LLM using multimodal message ===
        match self
            .llm_interface
            .chat_multimodal_with_history(user_msg, &core_history)
            .await
        {
            Ok(llm_response) => {
                let response_msg = AgentMessage::assistant(&llm_response.text);

                self.internal_state
                    .write()
                    .await
                    .push_message(response_msg.clone());

                self.internal_state
                    .write()
                    .await
                    .session
                    .increment_messages();

                let processing_time = start.elapsed().as_millis() as u64;

                Ok(AgentResponse {
                    message: response_msg,
                    tool_calls: vec![],
                    memory_context_used: false,
                    tools_used: vec![],
                    processing_time_ms: processing_time,
                })
            }
            Err(e) => Err(NeoMindError::Llm(format!("LLM processing failed: {}", e))),
        }
    }
}
impl Agent {
    /// Process a multimodal user message (text + images) with streaming response (returns AgentEvent stream).
    pub async fn process_multimodal_stream_events(
        &self,
        user_message: &str,
        images: Vec<String>, // Base64 data URLs
    ) -> Result<Pin<Box<dyn Stream<Item = AgentEvent> + Send>>> {
        self.process_multimodal_stream_events_with_safeguards(
            user_message,
            images,
            StreamSafeguards::default(),
        )
        .await
    }
}
impl Agent {
    /// Process a multimodal user message with streaming response and custom safeguards.
    pub async fn process_multimodal_stream_events_with_safeguards(
        &self,
        user_message: &str,
        images: Vec<String>, // Base64 data URLs
        safeguards: StreamSafeguards,
    ) -> Result<Pin<Box<dyn Stream<Item = AgentEvent> + Send>>> {
        tracing::debug!(
            message = %user_message,
            image_count = images.len(),
            "Agent::process_multimodal_stream_events starting"
        );

        let _lock = self.process_lock.lock().await;

        // Check if LLM is configured (required for multimodal)
        if !self.llm_interface.is_ready().await {
            // Fall back to simple response without LLM
            let (message, _, _) =
                process_fallback(&self.tools, &self.fallback_rules, user_message).await;
            self.internal_state
                .write()
                .await
                .push_message(message.clone());

            return Ok(Box::pin(async_stream::stream! {
                yield AgentEvent::content(message.content);
                yield AgentEvent::end();
            }));
        }

        match process_multimodal_stream_events_with_safeguards(
            self.llm_interface.clone(),
            self.internal_state.clone(),
            self.tools.clone(),
            user_message,
            images,
            safeguards,
            None,
            None,
        )
        .await
        {
            Ok(stream) => Ok(stream),
            Err(e) => {
                // On error, fall back to simple response
                tracing::error!(error = %e, "LLM multimodal stream error, using fallback");
                let (message, _, _) =
                    process_fallback(&self.tools, &self.fallback_rules, user_message).await;
                self.internal_state
                    .write()
                    .await
                    .push_message(message.clone());

                Ok(Box::pin(async_stream::stream! {
                    yield AgentEvent::content(message.content);
                    yield AgentEvent::end();
                }))
            }
        }
    }
}
impl Agent {
    /// Process with real LLM.
    ///
    /// ## Safeguards:
    /// - Maximum tool calls per request limited to MAX_TOOL_CALLS_PER_REQUEST
    /// - Tool result clearing for old messages (Anthropic-style)
    /// - Token limit configured in ChatConfig
    pub async fn process_stream_events(
        &self,
        user_message: &str,
        conversation_summary: Option<String>,
        summary_up_to_index: Option<u64>,
    ) -> Result<Pin<Box<dyn Stream<Item = AgentEvent> + Send>>> {
        self.process_stream_events_with_safeguards(
            user_message,
            conversation_summary,
            summary_up_to_index,
            StreamSafeguards::default(),
        )
        .await
    }
}
impl Agent {
    /// Process a user message with streaming response and custom safeguards (e.g., interrupt signal).
    pub async fn process_stream_events_with_safeguards(
        &self,
        user_message: &str,
        conversation_summary: Option<String>,
        summary_up_to_index: Option<u64>,
        safeguards: StreamSafeguards,
    ) -> Result<Pin<Box<dyn Stream<Item = AgentEvent> + Send>>> {
        // Add user message to history
        let user_msg = AgentMessage::user(user_message);
        self.internal_state.write().await.push_message(user_msg);

        // Set session ID on memory tool to avoid cross-session contamination
        self.tools
            .set_memory_session_id(self.session_id.clone())
            .await;

        // Check if LLM is configured
        if !self.llm_interface.is_ready().await {
            // Fall back to simple response
            let (message, _, _) =
                process_fallback(&self.tools, &self.fallback_rules, user_message).await;
            self.internal_state
                .write()
                .await
                .push_message(message.clone());

            // Return a single-item stream with the fallback response
            let content = message.content;
            return Ok(Box::pin(async_stream::stream! {
                yield AgentEvent::content(content);
                yield AgentEvent::end();
            }));
        }

        match process_stream_events_with_safeguards(
            self.llm_interface.clone(),
            self.internal_state.clone(),
            self.tools.clone(),
            user_message,
            safeguards,
            conversation_summary,
            summary_up_to_index,
        )
        .await
        {
            Ok(stream) => Ok(stream),
            Err(e) => {
                // On error, fall back to simple response
                tracing::error!(error = %e, "LLM stream error, using fallback");
                let (message, _, _) =
                    process_fallback(&self.tools, &self.fallback_rules, user_message).await;
                self.internal_state
                    .write()
                    .await
                    .push_message(message.clone());

                Ok(Box::pin(async_stream::stream! {
                    yield AgentEvent::content(message.content);
                    yield AgentEvent::end();
                }))
            }
        }
    }
}
impl Agent {
    /// Process a user message with streaming response (returns String stream).
    pub async fn process_stream(
        &self,
        user_message: &str,
    ) -> Result<Pin<Box<dyn Stream<Item = String> + Send>>> {
        let event_stream = self.process_stream_events(user_message, None, None).await?;
        Ok(events_to_string_stream(event_stream))
    }
}
