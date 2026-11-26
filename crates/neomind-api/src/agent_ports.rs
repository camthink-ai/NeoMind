//! Production host adapters for [`neomind_ai_agent`] ports.
//!
//! Wired from [`crate::ai_agent_runtime::HostedAiAgentRuntime::build`] so the
//! clean-slate kernel reads real telemetry/extensions, calls the instance
//! manager LLM, and delivers via MessageManager / DeviceService.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use neomind_ai_agent::error::{AgentError, Result as AgentResult};
use neomind_ai_agent::ports::{
    LlmBackendCapabilities, LlmImage, LlmPort, LlmRequest, LlmResponse, NotifyPort, SideEffectPort,
    SideEffectReceipt, SourceReadPort, SourceReadResult,
};
use neomind_ai_agent::runner::alert_from_model_text;
use neomind_ai_agent::service::{NotifyCounter, WriteCounter};
use neomind_core::eventbus::EventBus;
use neomind_core::extension::{CommandEffect, ExtensionRegistry};
use neomind_core::llm::backend::{sanitize_tool_name, ToolDefinition as CoreToolDefinition};
use neomind_core::llm::{GenerationParams, LlmError, LlmInput};
use neomind_core::message::{Content, ContentPart, MessageRole};
use neomind_core::Message;
use neomind_devices::{CommandDelivery, DeviceService, TimeSeriesStorage};
use neomind_messages::{Message as NotifyMessage, MessageManager, MessageSeverity};
use tokio_util::sync::CancellationToken;

use crate::automation::metric_publish::VirtualMetricPublisher;
use crate::server::state::ExtensionMetricsStorage;

/// Host services injected into the clean-slate runtime builder.
#[derive(Clone)]
pub struct HostAgentDeps {
    pub message_manager: Arc<MessageManager>,
    pub telemetry: Arc<TimeSeriesStorage>,
    pub device_service: Arc<DeviceService>,
    pub extension_registry: Arc<ExtensionRegistry>,
    pub extension_metrics: Arc<ExtensionMetricsStorage>,
    pub event_bus: Option<Arc<EventBus>>,
    pub metric_publisher: Option<Arc<VirtualMetricPublisher>>,
    pub memory_store: Option<Arc<neomind_storage::MarkdownMemoryStore>>,
    pub session_manager: Option<Arc<neomind_agent::SessionManager>>,
}

impl HostAgentDeps {
    pub fn new(
        message_manager: Arc<MessageManager>,
        telemetry: Arc<TimeSeriesStorage>,
        device_service: Arc<DeviceService>,
        extension_registry: Arc<ExtensionRegistry>,
        extension_metrics: Arc<ExtensionMetricsStorage>,
    ) -> Self {
        Self {
            message_manager,
            telemetry,
            device_service,
            extension_registry,
            extension_metrics,
            event_bus: None,
            metric_publisher: None,
            memory_store: None,
            session_manager: None,
        }
    }

    pub fn with_event_bus(mut self, event_bus: Arc<EventBus>) -> Self {
        self.event_bus = Some(event_bus);
        self
    }

    pub fn with_metric_publisher(mut self, publisher: Arc<VirtualMetricPublisher>) -> Self {
        self.metric_publisher = Some(publisher);
        self
    }

    pub fn with_memory_store(
        mut self,
        memory_store: Arc<neomind_storage::MarkdownMemoryStore>,
    ) -> Self {
        self.memory_store = Some(memory_store);
        self
    }

    pub fn with_session_manager(
        mut self,
        session_manager: Arc<neomind_agent::SessionManager>,
    ) -> Self {
        self.session_manager = Some(session_manager);
        self
    }
}

// ── LLM ─────────────────────────────────────────────────────────────────────

pub struct HostLlmPort {
    pub request_count: AtomicU64,
    /// Test hook: next `complete` fails like a backend 5xx (no local-brief fake).
    pub force_fail_next: AtomicBool,
    session_manager: Option<Arc<neomind_agent::SessionManager>>,
}

impl HostLlmPort {
    pub fn new() -> Self {
        Self {
            request_count: AtomicU64::new(0),
            force_fail_next: AtomicBool::new(false),
            session_manager: None,
        }
    }

    pub fn with_session_manager(session_manager: Arc<neomind_agent::SessionManager>) -> Self {
        Self {
            request_count: AtomicU64::new(0),
            force_fail_next: AtomicBool::new(false),
            session_manager: Some(session_manager),
        }
    }

    pub fn requests(&self) -> u64 {
        self.request_count.load(Ordering::SeqCst)
    }
}

impl Default for HostLlmPort {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl LlmPort for HostLlmPort {
    async fn resolve_backend(
        &self,
        backend_id: Option<&str>,
    ) -> AgentResult<LlmBackendCapabilities> {
        self.try_resolve(backend_id)
    }

    async fn complete(
        &self,
        request: LlmRequest,
        cancel: CancellationToken,
    ) -> AgentResult<LlmResponse> {
        self.request_count.fetch_add(1, Ordering::SeqCst);
        if self.force_fail_next.swap(false, Ordering::SeqCst) {
            return Err(AgentError::Llm("simulated backend 5xx".into()));
        }
        // Never fall back to LocalBriefLlm — generate failure must settle as fail.
        self.try_complete(&request, cancel).await
    }
}

impl HostLlmPort {
    fn try_resolve(&self, backend_id: Option<&str>) -> AgentResult<LlmBackendCapabilities> {
        let manager =
            neomind_agent::get_instance_manager().map_err(|e| AgentError::Llm(e.to_string()))?;
        let instance = match backend_id {
            Some(id) if id != "default" => manager
                .get_instance(id)
                .ok_or_else(|| AgentError::Llm(format!("backend '{id}' not found")))?,
            _ => manager
                .get_active_instance()
                .ok_or_else(|| AgentError::Llm("no active LLM backend".into()))?,
        };
        Ok(LlmBackendCapabilities {
            multimodal: instance.capabilities.supports_multimodal,
            tools: instance.capabilities.supports_tools,
            model: instance.model.clone(),
        })
    }

    async fn try_complete(
        &self,
        request: &LlmRequest,
        cancel: CancellationToken,
    ) -> AgentResult<LlmResponse> {
        let manager =
            neomind_agent::get_instance_manager().map_err(|e| AgentError::Llm(e.to_string()))?;
        let runtime = match request.backend_id.as_deref() {
            Some(id) if id != "default" => manager
                .get_runtime(id)
                .await
                .map_err(|e| AgentError::Llm(e.to_string()))?,
            _ => manager
                .get_active_runtime()
                .await
                .map_err(|e| AgentError::Llm(e.to_string()))?,
        };

        let params = GenerationParams {
            thinking_enabled: Some(false),
            max_tokens: Some(2048),
            ..Default::default()
        };

        // Resolve authorized tools from the live ToolRegistry when ToolPolicy enables tools.
        let (registry_opt, tool_defs, tool_name_map) = if request.tools_enabled {
            if let Some(ref sm) = self.session_manager {
                if let Some(registry) = sm.get_tool_registry().await {
                    let (defs, name_map) =
                        select_authorized_tools(&registry, request.allowed_tools.as_deref());
                    (Some(registry), defs, name_map)
                } else {
                    (None, Vec::new(), HashMap::new())
                }
            } else {
                (None, Vec::new(), HashMap::new())
            }
        } else {
            (None, Vec::new(), HashMap::new())
        };

        let mut system_text = request.system.clone();
        if !tool_defs.is_empty() {
            system_text.push_str("\n\n## Available Tools\n");
            for def in &tool_defs {
                system_text.push_str(&format!("- {}: {}\n", def.name, def.description));
            }
        }

        let image_parts = resolve_llm_image_parts(&request.images);
        let max_ctx = runtime.max_context_length();
        let user_text = fit_user_prompt_to_context(
            &request.user,
            system_text.len(),
            image_parts.len(),
            max_ctx,
            2048,
        );

        let initial_user_msg = if image_parts.is_empty() {
            Message::user(user_text)
        } else {
            let mut parts = Vec::with_capacity(1 + image_parts.len());
            parts.push(ContentPart::text(user_text));
            parts.extend(image_parts.clone());
            Message::new(MessageRole::User, Content::Parts(parts))
        };

        let mut messages = vec![Message::system(system_text.clone()), initial_user_msg];

        const MAX_TOOL_ROUNDS: usize = 6;
        let max_rounds = if tool_defs.is_empty() {
            1
        } else {
            MAX_TOOL_ROUNDS
        };

        let mut final_text = String::new();

        for round in 0..max_rounds {
            let round_tools = if !tool_defs.is_empty() && round + 1 < max_rounds {
                Some(tool_defs.clone())
            } else {
                None
            };

            let input = LlmInput {
                messages: messages.clone(),
                params: params.clone(),
                model: None,
                stream: false,
                tools: round_tools.clone(),
            };

            let first_result = tokio::select! {
                _ = cancel.cancelled() => return Err(AgentError::Cancelled),
                result = runtime.generate(input) => result,
            };

            let output = match first_result {
                Ok(out) => out,
                Err(LlmError::ContextOverflow {
                    prompt_tokens,
                    max_context,
                }) => {
                    tracing::warn!(
                        prompt_tokens,
                        max_context,
                        round,
                        images = image_parts.len(),
                        "Agent LLM context overflow; retrying with compacted prompt"
                    );
                    let retry_imgs = if max_context < 8192 {
                        Vec::new()
                    } else {
                        image_parts.iter().take(1).cloned().collect()
                    };
                    let compact_text = fit_user_prompt_to_context(
                        &request.user,
                        system_text.len(),
                        retry_imgs.len(),
                        max_context,
                        2048,
                    );
                    let compact_user = if retry_imgs.is_empty() {
                        Message::user(compact_text)
                    } else {
                        let mut parts = Vec::with_capacity(1 + retry_imgs.len());
                        parts.push(ContentPart::text(compact_text));
                        parts.extend(retry_imgs);
                        Message::new(MessageRole::User, Content::Parts(parts))
                    };
                    let retry_input = LlmInput {
                        messages: vec![Message::system(system_text.clone()), compact_user],
                        params: params.clone(),
                        model: None,
                        stream: false,
                        tools: None,
                    };
                    tokio::select! {
                        _ = cancel.cancelled() => return Err(AgentError::Cancelled),
                        result = runtime.generate(retry_input) => {
                            result.map_err(|e| AgentError::Llm(e.to_string()))?
                        }
                    }
                }
                Err(e) => return Err(AgentError::Llm(e.to_string())),
            };

            let calls = if round_tools.is_some() {
                extract_authorized_tool_calls(&output, &tool_name_map)
            } else {
                Vec::new()
            };

            if calls.is_empty() {
                final_text = output.text;
                break;
            }

            let Some(ref registry) = registry_opt else {
                final_text = output.text;
                break;
            };

            let mut call_summaries = Vec::with_capacity(calls.len());
            let mut tool_results = Vec::with_capacity(calls.len());

            for (orig_name, args) in calls {
                call_summaries.push(format!("{orig_name}({args})"));

                // Optional command allowlist check for shell tool.
                if orig_name == "shell" && !request.allowed_commands.is_empty() {
                    let cmd_str = args
                        .get("command")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .trim();
                    let first_tok = cmd_str.split_whitespace().next().unwrap_or("");
                    let permitted = request
                        .allowed_commands
                        .iter()
                        .any(|allowed| allowed == first_tok || cmd_str.starts_with(allowed.as_str()));
                    if !permitted {
                        tool_results.push(format!(
                            "### Tool `{orig_name}` rejected\nCommand `{first_tok}` is not in allowed_commands."
                        ));
                        continue;
                    }
                }

                let exec_res = tokio::select! {
                    _ = cancel.cancelled() => return Err(AgentError::Cancelled),
                    res = tokio::time::timeout(
                        Duration::from_secs(30),
                        registry.execute(&orig_name, args),
                    ) => res,
                };

                let formatted = match exec_res {
                    Ok(Ok(out)) => {
                        let raw_val = if out.success {
                            out.data.to_string()
                        } else {
                            serde_json::json!({
                                "success": false,
                                "error": out.error,
                                "data": out.data,
                            })
                            .to_string()
                        };
                        format!(
                            "### Tool `{orig_name}` result\n{}",
                            truncate_tool_output(&raw_val, 6_000)
                        )
                    }
                    Ok(Err(e)) => format!("### Tool `{orig_name}` error\n{e}"),
                    Err(_) => format!("### Tool `{orig_name}` error\nTimed out after 30s"),
                };
                tool_results.push(formatted);
            }

            messages.push(Message::assistant(format!(
                "Calling tools: {}",
                call_summaries.join(", ")
            )));
            let follow_up = if round + 2 >= max_rounds {
                format!(
                    "## Tool Results\n{}\n\nYou have reached the final round. Synthesize your final answer now without calling more tools.",
                    tool_results.join("\n\n")
                )
            } else {
                format!(
                    "## Tool Results\n{}\n\nUse these tool results to continue or provide your final answer.",
                    tool_results.join("\n\n")
                )
            };
            messages.push(Message::user(follow_up));
            final_text = output.text;
        }

        Ok(LlmResponse {
            text: final_text.clone(),
            alert_criteria_met: alert_from_model_text(&final_text),
        })
    }
}

fn select_authorized_tools(
    registry: &neomind_agent::toolkit::ToolRegistry,
    allowed_tools: Option<&[String]>,
) -> (Vec<CoreToolDefinition>, HashMap<String, String>) {
    let mut defs = Vec::new();
    let mut name_map = HashMap::new();
    for def in registry.definitions() {
        if registry.is_disabled(&def.name) {
            continue;
        }
        if let Some(allow) = allowed_tools {
            if !allow.is_empty() && !allow.iter().any(|a| a.eq_ignore_ascii_case(&def.name)) {
                continue;
            }
        }
        let sanitized = sanitize_tool_name(&def.name);
        name_map.insert(def.name.clone(), def.name.clone());
        name_map.insert(sanitized.clone(), def.name.clone());
        defs.push(CoreToolDefinition {
            name: sanitized,
            description: def.description,
            parameters: def.parameters,
        });
    }
    (defs, name_map)
}

/// Extract tool calls from native `LlmOutput.tool_calls` or text-protocol fallbacks
/// (`<tool_call>...</tool_call>` / JSON array), filtering strictly to authorized tool names.
fn extract_authorized_tool_calls(
    output: &neomind_core::llm::LlmOutput,
    name_map: &HashMap<String, String>,
) -> Vec<(String, serde_json::Value)> {
    let mut calls = Vec::new();

    if let Some(ref native) = output.tool_calls {
        for tc in native {
            if let Some(name) = tc.get("name").and_then(|v| v.as_str()) {
                if let Some(orig) = name_map.get(name) {
                    let args = tc
                        .get("arguments")
                        .cloned()
                        .unwrap_or_else(|| serde_json::json!({}));
                    let args = match args {
                        serde_json::Value::String(s) => {
                            serde_json::from_str(&s).unwrap_or_else(|_| serde_json::json!({}))
                        }
                        other => other,
                    };
                    calls.push((orig.clone(), args));
                }
            }
        }
    }

    if !calls.is_empty() {
        return calls;
    }

    // Fallback: parse text-protocol tool calls emitted by models without native tool_calls.
    let text = output.text.trim();
    if text.is_empty() {
        return calls;
    }

    // 1) <tool_call>{...}</tool_call> blocks
    let mut search = text;
    while let Some(start) = search.find("<tool_call>") {
        let after = &search[start + "<tool_call>".len()..];
        let body = if let Some(end) = after.find("</tool_call>") {
            search = &after[end + "</tool_call>".len()..];
            after[..end].trim()
        } else {
            search = "";
            after.trim()
        };
        if let Some(val) = parse_first_json_value(body) {
            collect_calls_from_json(&val, name_map, &mut calls);
        }
    }
    if !calls.is_empty() {
        return calls;
    }

    // 2) JSON array [{"name": "...", "arguments": {...}}] or single {"name": "...", "arguments": {...}}
    if let Ok(val) = serde_json::from_str::<serde_json::Value>(text) {
        collect_calls_from_json(&val, name_map, &mut calls);
    } else if let Some(i) = text.find('[') {
        if let Some(j) = text.rfind(']') {
            if j > i {
                if let Ok(val) = serde_json::from_str::<serde_json::Value>(&text[i..=j]) {
                    collect_calls_from_json(&val, name_map, &mut calls);
                }
            }
        }
    }
    calls
}

fn parse_first_json_value(s: &str) -> Option<serde_json::Value> {
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(s) {
        return Some(v);
    }
    let start = s.find('{')?;
    let bytes = s.as_bytes();
    let mut depth = 0usize;
    let mut in_string = false;
    let mut escaped = false;
    for (idx, &b) in bytes.iter().enumerate().skip(start) {
        if in_string {
            if escaped {
                escaped = false;
            } else if b == b'\\' {
                escaped = true;
            } else if b == b'"' {
                in_string = false;
            }
        } else {
            match b {
                b'"' => in_string = true,
                b'{' => depth += 1,
                b'}' => {
                    depth = depth.saturating_sub(1);
                    if depth == 0 {
                        return serde_json::from_str::<serde_json::Value>(&s[start..=idx]).ok();
                    }
                }
                _ => {}
            }
        }
    }
    None
}

fn collect_calls_from_json(
    val: &serde_json::Value,
    name_map: &HashMap<String, String>,
    out: &mut Vec<(String, serde_json::Value)>,
) {
    match val {
        serde_json::Value::Array(arr) => {
            for item in arr {
                collect_calls_from_json(item, name_map, out);
            }
        }
        serde_json::Value::Object(obj) => {
            if let Some(name) = obj.get("name").and_then(|v| v.as_str()) {
                if let Some(orig) = name_map.get(name) {
                    let args = obj
                        .get("arguments")
                        .or_else(|| obj.get("parameters"))
                        .cloned()
                        .unwrap_or_else(|| serde_json::json!({}));
                    let args = match args {
                        serde_json::Value::String(s) => {
                            serde_json::from_str(&s).unwrap_or_else(|_| serde_json::json!({}))
                        }
                        other => other,
                    };
                    out.push((orig.clone(), args));
                }
            }
        }
        _ => {}
    }
}

fn truncate_tool_output(s: &str, max_chars: usize) -> String {
    let total = s.chars().count();
    if total <= max_chars {
        return s.to_string();
    }
    let kept: String = s.chars().take(max_chars).collect();
    let dropped = total - max_chars;
    format!("{kept}... [truncated {dropped} chars]")
}

fn resolve_llm_image_parts(images: &[LlmImage]) -> Vec<ContentPart> {
    if images.is_empty() {
        return Vec::new();
    }
    let data_dir = neomind_core::paths::data_dir();
    let mut parts = Vec::with_capacity(images.len());
    for img in images {
        if let Some(ref mime) = img.mime_type {
            if !img.data.is_empty() {
                parts.push(ContentPart::image_base64(img.data.clone(), mime.clone()));
            }
            continue;
        }
        let url = img.data.trim();
        if url.starts_with("/api/images/") {
            if let Some(data_url) =
                neomind_devices::image_storage::resolve_internal_image_to_data_url(url, &data_dir)
            {
                if let Some((meta, b64)) = data_url
                    .strip_prefix("data:")
                    .and_then(|rest| rest.split_once(','))
                {
                    let mime = meta.strip_suffix(";base64").unwrap_or(meta);
                    if !mime.is_empty() && !b64.is_empty() {
                        parts.push(ContentPart::image_base64(b64.to_string(), mime.to_string()));
                        continue;
                    }
                }
            } else {
                tracing::warn!(url = %url, "Skipping unreadable /api/images/ attachment for agent LLM");
            }
        } else if url.starts_with("http://") || url.starts_with("https://") {
            parts.push(ContentPart::image_url(url.to_string()));
        }
    }
    parts
}

/// Fit the user prompt text comfortably inside `max_context` tokens, accounting
/// for system prompt, output tokens, and attached vision tokens.
fn fit_user_prompt_to_context(
    user: &str,
    system_chars: usize,
    image_count: usize,
    max_context_tokens: usize,
    output_reserve_tokens: usize,
) -> String {
    if max_context_tokens == 0 {
        return user.to_string();
    }
    // Conservative token estimate (mixed CJK/JSON can be ~1.5-2 chars/token).
    let system_tokens = (system_chars / 2) + 256;
    let image_tokens = image_count.saturating_mul(1536);
    let reserved = output_reserve_tokens
        .saturating_add(system_tokens)
        .saturating_add(image_tokens);
    let available_tokens = max_context_tokens.saturating_sub(reserved).max(512);
    let max_chars = available_tokens.saturating_mul(2);
    let total_chars = user.chars().count();
    if total_chars <= max_chars {
        return user.to_string();
    }
    let kept: String = user.chars().take(max_chars).collect();
    let dropped = total_chars - max_chars;
    format!("{kept}\n\n... [prompt truncated by {dropped} chars to fit {max_context_tokens}-token context]")
}

// ── Source reads ────────────────────────────────────────────────────────────

pub struct HostSourceReadPort {
    deps: HostAgentDeps,
    pub read_count: AtomicU64,
}

impl HostSourceReadPort {
    pub fn new(deps: HostAgentDeps) -> Self {
        Self {
            deps,
            read_count: AtomicU64::new(0),
        }
    }

    pub fn reads(&self) -> u64 {
        self.read_count.load(Ordering::SeqCst)
    }

    /// Fail closed: only ReadOnly commands may run from the input pipeline.
    async fn ensure_readonly_command(
        &self,
        extension_id: &str,
        operation: &str,
    ) -> AgentResult<()> {
        let info = self
            .deps
            .extension_registry
            .get_info(extension_id)
            .await
            .ok_or_else(|| AgentError::Source {
                key: extension_id.into(),
                reason: format!("extension '{extension_id}' not registered"),
            })?;
        let cmd = info.commands.iter().find(|c| c.name == operation);
        let effect = match cmd {
            Some(c) => c.effect.unwrap_or(CommandEffect::Mutating),
            None => {
                // Unknown command — not a declared metric either at this point.
                return Err(AgentError::Source {
                    key: extension_id.into(),
                    reason: format!(
                        "refusing unknown/write operation '{operation}' on extension '{extension_id}' (input path is read-only)"
                    ),
                });
            }
        };
        if !matches!(effect, CommandEffect::ReadOnly) {
            return Err(AgentError::Source {
                key: extension_id.into(),
                reason: format!(
                    "refusing non-readonly operation '{operation}' (effect={effect:?}) on input path"
                ),
            });
        }
        Ok(())
    }
}

#[async_trait]
impl SourceReadPort for HostSourceReadPort {
    async fn read_extension(
        &self,
        extension_id: &str,
        operation: &str,
        params: &serde_json::Value,
        timeout_secs: u32,
    ) -> AgentResult<SourceReadResult> {
        self.read_count.fetch_add(1, Ordering::SeqCst);
        let source_id = format!("extension:{extension_id}");
        let timeout = Duration::from_secs(timeout_secs.max(1) as u64);
        let now_ms = chrono::Utc::now().timestamp_millis();
        let freshness = params.get("_freshness").cloned();

        // Prefer durable metric storage when the operation names a metric field.
        if let Ok(Some(point)) = self
            .deps
            .extension_metrics
            .query_latest(&source_id, operation)
            .await
        {
            let value = apply_ext_freshness_to_point(&freshness, &point, now_ms, operation);
            return Ok(SourceReadResult {
                source_id: source_id.clone(),
                binding_proof: format!("{source_id}:{operation}"),
                observed_at: Some(point.timestamp),
                // Never echo the *requested* window as actual coverage.
                valid_window: None,
                value,
            });
        }

        // Live registry metrics (read-only by nature).
        let metrics = tokio::time::timeout(
            timeout,
            self.deps
                .extension_registry
                .get_current_metrics(extension_id),
        )
        .await
        .map_err(|_| AgentError::Source {
            key: extension_id.into(),
            reason: "extension metrics timed out".into(),
        })?;
        if let Some(m) = metrics.iter().find(|m| m.name == operation) {
            return Ok(SourceReadResult {
                source_id: source_id.clone(),
                binding_proof: format!("{source_id}:{operation}:live"),
                observed_at: Some(m.timestamp),
                valid_window: None,
                value: serde_json::to_value(m).unwrap_or(serde_json::Value::Null),
            });
        }

        // Command path — must be declared ReadOnly (blocks dry_run writes).
        self.ensure_readonly_command(extension_id, operation)
            .await?;

        let result = tokio::time::timeout(
            timeout,
            self.deps
                .extension_registry
                .execute_command(extension_id, operation, params),
        )
        .await
        .map_err(|_| AgentError::Source {
            key: extension_id.into(),
            reason: "extension command timed out".into(),
        })?
        .map_err(|e| AgentError::Source {
            key: extension_id.into(),
            reason: e.to_string(),
        })?;

        // Call time is fetch time, not observation time. Task params must never
        // forge `_invoke_is_sample` — only payload obs time (or host-trusted
        // registry metadata) may set observed_at.
        let observed_at = parse_observation_time(&result).or_else(|| {
            if host_trusted_invoke_is_sample(&self.deps, extension_id, operation) {
                Some(now_ms)
            } else {
                None
            }
        });

        // Do not echo the *requested* freshness window as actual coverage —
        // LookbackWindow classification needs real point timestamps.
        Ok(SourceReadResult {
            source_id,
            binding_proof: format!("extension:{extension_id}:{operation}:cmd"),
            observed_at,
            valid_window: None,
            value: result,
        })
    }

    async fn read_device_metric(
        &self,
        device_id: &str,
        field: &str,
        params: &serde_json::Value,
        timeout_secs: u32,
    ) -> AgentResult<SourceReadResult> {
        self.read_count.fetch_add(1, Ordering::SeqCst);
        let source_key = format!("device:{device_id}");
        let timeout = Duration::from_secs(timeout_secs.max(1) as u64);
        let now_ms = chrono::Utc::now().timestamp_millis();
        let freshness = params.get("_freshness").cloned();

        if field == "*" {
            let _ = self.deps.telemetry.flush();
            let metrics =
                tokio::time::timeout(timeout, self.deps.telemetry.list_metrics(&source_key))
                    .await
                    .map_err(|_| AgentError::Source {
                        key: device_id.into(),
                        reason: "list_metrics timed out".into(),
                    })?
                    .map_err(|e| AgentError::Source {
                        key: device_id.into(),
                        reason: e.to_string(),
                    })?;
            let metric_refs: Vec<&str> = metrics.iter().map(|s| s.as_str()).collect();
            let batch = tokio::time::timeout(
                timeout,
                self.deps.telemetry.latest_batch(&source_key, &metric_refs),
            )
            .await
            .map_err(|_| AgentError::Source {
                key: device_id.into(),
                reason: "latest_batch timed out".into(),
            })?
            .map_err(|e| AgentError::Source {
                key: device_id.into(),
                reason: e.to_string(),
            })?;
            let observed = batch.values().map(|p| p.timestamp).max();
            return Ok(SourceReadResult {
                source_id: source_key.clone(),
                binding_proof: format!("{source_key}:*"),
                observed_at: observed,
                valid_window: lookback_window_from_freshness(&freshness, now_ms),
                value: serde_json::to_value(&batch).unwrap_or(serde_json::json!({})),
            });
        }

        // Windowed tasks must range-query — never substitute a lone latest.
        self.read_telemetry_metric(&source_key, device_id, field, params, timeout_secs)
            .await
    }

    async fn read_upstream_ai(
        &self,
        agent_id: &str,
        field: &str,
        params: &serde_json::Value,
        timeout_secs: u32,
    ) -> AgentResult<SourceReadResult> {
        self.read_count.fetch_add(1, Ordering::SeqCst);
        let source_key = format!("ai:{agent_id}");
        self.read_telemetry_metric(&source_key, &source_key, field, params, timeout_secs)
            .await
    }
}

impl HostSourceReadPort {
    async fn read_telemetry_metric(
        &self,
        source_key: &str,
        err_key: &str,
        field: &str,
        params: &serde_json::Value,
        timeout_secs: u32,
    ) -> AgentResult<SourceReadResult> {
        let timeout = Duration::from_secs(timeout_secs.max(1) as u64);
        let now_ms = chrono::Utc::now().timestamp_millis();
        let freshness = params.get("_freshness").cloned();

        // Windowed tasks must range-query — never substitute a lone latest.
        if let Some(lookback_secs) = freshness
            .as_ref()
            .and_then(|f| f.get("lookback_secs"))
            .and_then(|v| v.as_u64())
            .or_else(|| params.get("lookback_secs").and_then(|v| v.as_u64()))
        {
            let max_points = freshness
                .as_ref()
                .and_then(|f| f.get("max_points"))
                .and_then(|v| v.as_u64())
                .unwrap_or(60) as usize;
            // Evidence contract is always ms; storage timestamps are usually
            // Unix seconds (MQTT/webhook/API). Peek latest to pick query unit.
            let window_ms = (
                now_ms.saturating_sub((lookback_secs as i64).saturating_mul(1000)),
                now_ms,
            );
            let _ = self.deps.telemetry.flush();
            let sample_ts = self
                .deps
                .telemetry
                .latest(source_key, field)
                .await
                .ok()
                .flatten()
                .map(|p| p.timestamp);
            let (start_q, end_q) = storage_range_for_series(lookback_secs, now_ms, sample_ts);
            let points = tokio::time::timeout(
                timeout,
                self.deps.telemetry.query_limited(
                    source_key,
                    field,
                    start_q,
                    end_q,
                    Some(max_points),
                ),
            )
            .await
            .map_err(|_| AgentError::Source {
                key: err_key.into(),
                reason: "query_limited timed out".into(),
            })?
            .map_err(|e| AgentError::Source {
                key: err_key.into(),
                reason: e.to_string(),
            })?;
            let observed = points.last().map(|p| normalize_ts_to_ms(p.timestamp));
            let series: Vec<serde_json::Value> = points
                .iter()
                .filter_map(|p| serde_json::to_value(p).ok())
                .collect();
            return Ok(SourceReadResult {
                source_id: source_key.to_string(),
                binding_proof: format!("{source_key}:{field}:lookback={lookback_secs}"),
                observed_at: observed,
                valid_window: Some(window_ms),
                value: serde_json::json!({ "points": series }),
            });
        }

        let _ = self.deps.telemetry.flush();
        let point = tokio::time::timeout(timeout, self.deps.telemetry.latest(source_key, field))
            .await
            .map_err(|_| AgentError::Source {
                key: err_key.into(),
                reason: "latest timed out".into(),
            })?
            .map_err(|e| AgentError::Source {
                key: err_key.into(),
                reason: e.to_string(),
            })?
            .ok_or_else(|| AgentError::Source {
                key: err_key.into(),
                reason: format!("no data for {source_key}:{field}"),
            })?;

        Ok(SourceReadResult {
            source_id: source_key.to_string(),
            binding_proof: format!("{source_key}:{field}"),
            observed_at: Some(point.timestamp),
            valid_window: lookback_window_from_freshness(&freshness, now_ms),
            value: serde_json::to_value(&point).unwrap_or(serde_json::Value::Null),
        })
    }
}

fn lookback_window_from_freshness(
    freshness: &Option<serde_json::Value>,
    now_ms: i64,
) -> Option<(i64, i64)> {
    let f = freshness.as_ref()?;
    let kind = f.get("kind").and_then(|v| v.as_str())?;
    match kind {
        "lookback_window" | "latest_within" => {
            let secs = f
                .get("lookback_secs")
                .or_else(|| f.get("max_age_secs"))
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            let ms = (secs as i64).saturating_mul(1000);
            Some((now_ms.saturating_sub(ms), now_ms))
        }
        _ => None,
    }
}

/// Host-trusted invoke-is-sample declaration. Task params are never consulted.
/// Reserved for future registry metadata; currently always false (fail closed).
fn host_trusted_invoke_is_sample(
    _deps: &HostAgentDeps,
    _extension_id: &str,
    _operation: &str,
) -> bool {
    false
}

/// Device/MQTT/API telemetry uses Unix seconds; some tests/writers use ms.
/// Pick storage query bounds from a sample timestamp when available.
fn storage_range_for_series(lookback_secs: u64, now_ms: i64, sample_ts: Option<i64>) -> (i64, i64) {
    let start_ms = now_ms.saturating_sub((lookback_secs as i64).saturating_mul(1000));
    match sample_ts {
        Some(ts) if ts >= 1_000_000_000_000 => (start_ms, now_ms),
        _ => {
            let end_s = now_ms / 1000;
            let start_s = end_s.saturating_sub(lookback_secs as i64);
            (start_s, end_s)
        }
    }
}

fn normalize_ts_to_ms(ts: i64) -> i64 {
    if ts < 1_000_000_000_000 {
        ts.saturating_mul(1000)
    } else {
        ts
    }
}

/// Pull observation time from a command/metric payload. Call time is never
/// implied — missing keys yield `None` (strict LatestWithin → stale/missing).
fn parse_observation_time(value: &serde_json::Value) -> Option<i64> {
    const KEYS: &[&str] = &[
        "observed_at",
        "observation_time",
        "timestamp",
        "time",
        "as_of",
        "updated_at",
        "sampled_at",
    ];
    for key in KEYS {
        if let Some(n) = value.get(*key).and_then(|v| v.as_i64()) {
            return Some(n);
        }
        if let Some(n) = value.get(*key).and_then(|v| v.as_f64()).map(|f| f as i64) {
            return Some(n);
        }
    }
    if let Some(inner) = value.get("value").or_else(|| value.get("data")) {
        if let Some(n) = parse_observation_time(inner) {
            return Some(n);
        }
    }
    None
}

fn apply_ext_freshness_to_point(
    _freshness: &Option<serde_json::Value>,
    point: &neomind_devices::telemetry::DataPoint,
    _now_ms: i64,
    _operation: &str,
) -> serde_json::Value {
    serde_json::to_value(point)
        .unwrap_or_else(|_| serde_json::json!({ "timestamp": point.timestamp }))
}

// ── Notify ──────────────────────────────────────────────────────────────────

pub struct HostNotifyPort {
    message_manager: Arc<MessageManager>,
    pub deliver_count: AtomicU64,
}

impl HostNotifyPort {
    pub fn new(message_manager: Arc<MessageManager>) -> Self {
        Self {
            message_manager,
            deliver_count: AtomicU64::new(0),
        }
    }

    pub fn deliveries(&self) -> u64 {
        self.deliver_count.load(Ordering::SeqCst)
    }
}

impl NotifyCounter for HostNotifyPort {
    fn count(&self) -> u64 {
        self.deliveries()
    }
}

#[async_trait]
impl NotifyPort for HostNotifyPort {
    async fn deliver(
        &self,
        channel: &str,
        subject: &str,
        body: &str,
        execution_id: &str,
    ) -> AgentResult<()> {
        self.deliver_count.fetch_add(1, Ordering::SeqCst);
        let mut msg = NotifyMessage::new(
            "agent",
            MessageSeverity::Info,
            subject.to_string(),
            body.to_string(),
            format!("agent-exec:{execution_id}"),
        );
        msg.source_type = "agent".into();
        msg.target_channels = Some(vec![channel.to_string()]);
        msg.metadata = Some(serde_json::json!({ "execution_id": execution_id }));
        self.message_manager
            .create_message(msg)
            .await
            .map_err(|e| AgentError::Delivery(e.to_string()))?;
        Ok(())
    }
}

// ── Side effects ────────────────────────────────────────────────────────────

pub struct HostSideEffectPort {
    device_service: Arc<DeviceService>,
    extension_registry: Arc<ExtensionRegistry>,
    pub invoke_count: AtomicU64,
}

impl HostSideEffectPort {
    pub fn new(
        device_service: Arc<DeviceService>,
        extension_registry: Arc<ExtensionRegistry>,
    ) -> Self {
        Self {
            device_service,
            extension_registry,
            invoke_count: AtomicU64::new(0),
        }
    }

    pub fn invokes(&self) -> u64 {
        self.invoke_count.load(Ordering::SeqCst)
    }
}

impl WriteCounter for HostSideEffectPort {
    fn count(&self) -> u64 {
        self.invokes()
    }
}

#[async_trait]
impl SideEffectPort for HostSideEffectPort {
    async fn invoke(
        &self,
        destination: &str,
        action: &str,
        params: &serde_json::Value,
        _execution_id: &str,
    ) -> AgentResult<SideEffectReceipt> {
        self.invoke_count.fetch_add(1, Ordering::SeqCst);

        if let Some(ext_id) = destination.strip_prefix("extension:") {
            // Extension Ok = Accepted, not device ACK.
            self.extension_registry
                .execute_command(ext_id, action, params)
                .await
                .map_err(|e| AgentError::Other(e.to_string()))?;
            return Ok(SideEffectReceipt::Accepted);
        }

        let device_id = destination.strip_prefix("device:").unwrap_or(destination);
        let map = json_to_string_map(params);

        if self.device_service.get_device(device_id).is_some() {
            let (_value, delivery) = self
                .device_service
                .send_command_reported(device_id, action, map)
                .await
                .map_err(|e| AgentError::Other(e.to_string()))?;
            return Ok(receipt_from_command_delivery(delivery));
        }

        if self
            .extension_registry
            .get_info(destination)
            .await
            .is_some()
        {
            self.extension_registry
                .execute_command(destination, action, params)
                .await
                .map_err(|e| AgentError::Other(e.to_string()))?;
            return Ok(SideEffectReceipt::Accepted);
        }

        let (_value, delivery) = self
            .device_service
            .send_command_reported(device_id, action, map)
            .await
            .map_err(|e| AgentError::Other(e.to_string()))?;
        Ok(receipt_from_command_delivery(delivery))
    }
}

fn receipt_from_command_delivery(delivery: CommandDelivery) -> SideEffectReceipt {
    match delivery {
        CommandDelivery::DeviceAcknowledged | CommandDelivery::PhysicalEffectVerified => {
            SideEffectReceipt::Confirmed
        }
        CommandDelivery::AdapterAccepted { .. } | CommandDelivery::ExtensionAccepted => {
            SideEffectReceipt::Accepted
        }
    }
}

fn json_to_string_map(params: &serde_json::Value) -> HashMap<String, serde_json::Value> {
    match params.as_object() {
        Some(map) => map.iter().map(|(k, v)| (k.clone(), v.clone())).collect(),
        None => HashMap::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use neomind_core::llm::{FinishReason, LlmOutput};

    #[test]
    fn extract_authorized_tool_calls_accepts_native_and_filters_unlisted() {
        let mut name_map = HashMap::new();
        name_map.insert("shell".to_string(), "shell".to_string());
        name_map.insert("web_search".to_string(), "web_search".to_string());

        let out = LlmOutput {
            text: String::new(),
            finish_reason: FinishReason::ToolCalls,
            usage: None,
            thinking: None,
            tool_calls: Some(vec![
                serde_json::json!({"id": "1", "name": "shell", "arguments": {"command": "uname -a"}}),
                serde_json::json!({"id": "2", "name": "forbidden_tool", "arguments": {}}),
            ]),
        };

        let calls = extract_authorized_tool_calls(&out, &name_map);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].0, "shell");
        assert_eq!(calls[0].1["command"], "uname -a");
    }

    #[test]
    fn extract_authorized_tool_calls_parses_text_tool_call_fallback() {
        let mut name_map = HashMap::new();
        name_map.insert("web_search".to_string(), "web_search".to_string());

        let out = LlmOutput {
            text: r#"<tool_call>{"name": "web_search", "arguments": {"query": "weather"}}</tool_call>"#.into(),
            finish_reason: FinishReason::Stop,
            usage: None,
            thinking: None,
            tool_calls: None,
        };

        let calls = extract_authorized_tool_calls(&out, &name_map);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].0, "web_search");
        assert_eq!(calls[0].1["query"], "weather");
    }
}
