//! AI Agent executor - runs agents and records decision processes.

#![allow(clippy::too_many_arguments)]

use crate::llm_backends::{OllamaConfig, OllamaRuntime};
use futures::future::join_all;
use futures::FutureExt;
use neomind_core::llm::backend::LlmRuntime;
use neomind_core::{
    message::{Content, ContentPart, Message, MessageRole},
    EventBus, MetricValue, NeoMindEvent,
};
use neomind_devices::DeviceService;

#[cfg(feature = "cloud")]
use crate::llm_backends::{CloudConfig, CloudRuntime};
use neomind_messages::MessageManager;
use neomind_storage::{
    AgentExecutionRecord, AgentResource, AgentStore, AgentToolConfig, AiAgent, DataCollected,
    Decision, DecisionProcess, ExecutionRecord, ExecutionResult as StorageExecutionResult,
    ExecutionStatus, GeneratedReport, LlmBackendStore, MarkdownMemoryStore, ReasoningStep,
    ResourceType,
};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{RwLock, Semaphore};

// Import DataSourceId for type-safe extension metric queries
use neomind_core::datasource::DataSourceId;

use crate::agent::types::LlmBackend;
use crate::error::{NeoMindError, Result as AgentResult};

/// Internal representation of image content for multimodal LLM messages.
pub(crate) enum ImageContent {
    Url(String),
    Base64(String, String), // (data, mime_type)
}

/// Intermediate data from the tool execution loop, passed to result construction.
pub(crate) struct ToolCallRecord {
    pub(crate) name: String,
    pub(crate) input: serde_json::Value,
    pub(crate) result: crate::toolkit::ToolResult,
}

pub(crate) struct RoundData {
    pub(crate) thought: Option<String>,
    pub(crate) tool_calls: Vec<ToolCallRecord>,
}

/// Why the tool-calling loop stopped. Set at every exit path in `run_tool_loop`
/// and surfaced via `ToolLoopOutput::stop_reason`, so callers (journaling,
/// metrics, debugging) know the reason without parsing sentinel strings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StopReason {
    /// LLM produced a final text answer (no further tool calls).
    NaturalCompletion,
    /// Hit the round budget (`max_rounds`); Phase 2 synthesized a summary.
    MaxRounds,
    /// StuckDetector flagged a pathological loop pattern.
    Stuck,
    /// Every tool call this round was a cross-round duplicate (results in hand).
    AllDuplicate,
    /// LLM generation failed (after transient retries).
    LlmError,
    /// Runtime shutdown — tool-concurrency semaphore closed.
    Cancelled,
}

impl StopReason {
    /// Stable machine label for journaling / telemetry.
    pub(crate) fn label(&self) -> &'static str {
        match self {
            StopReason::NaturalCompletion => "natural-completion",
            StopReason::MaxRounds => "max-rounds",
            StopReason::Stuck => "stuck",
            StopReason::AllDuplicate => "all-duplicate",
            StopReason::LlmError => "llm-error",
            StopReason::Cancelled => "cancelled",
        }
    }
}

pub(crate) struct ToolLoopOutput {
    pub(crate) final_text: String,
    /// Why the loop ended (replaces ad-hoc sentinel-string comparisons).
    pub(crate) stop_reason: StopReason,
    pub(crate) all_tool_results: Vec<crate::toolkit::ToolResult>,
    /// (thought, tool_calls) per round
    pub(crate) round_data_list_raw: Vec<(Option<String>, Vec<ToolCallRecord>)>,
    /// Captured LLM error if `generate()` failed during the loop.
    /// Used by `execute_with_tools` to surface the real error cause instead
    /// of a generic "tool-calling failed" message.
    pub(crate) last_llm_error: Option<neomind_core::llm::backend::LlmError>,
}

/// Outcome of intra-round + cross-round deduplication.
#[derive(PartialEq)]
pub(crate) enum DedupOutcome {
    /// Some tool calls survived deduplication.
    /// Contains the count and signatures of cross-round duplicates that were dropped.
    HasNew { skipped_cross_round: Vec<String> },
    /// All tool calls were duplicates.
    AllDuplicate,
}

/// Hard limit for a single tool result (128 KB).
/// Applied both during tool-loop message construction and Phase 2 summary.
pub(crate) const TOOL_RESULT_MAX_LEN: usize = 131_072;

/// Configuration for the tool loop, varying by execution mode.
pub(crate) struct ToolLoopConfig {
    /// Maximum LLM call rounds
    pub(crate) max_rounds: usize,
    /// Recommended tool names (None = no recommendation, all tools available).
    /// Not a whitelist — just prompt guidance for the LLM.
    pub(crate) recommended_tools: Option<Vec<String>>,
    /// Whether this is Focused+ mode (Focused with tool chaining enabled)
    pub(crate) is_focused_plus: bool,
}

impl ToolLoopConfig {
    fn free() -> Self {
        let max_rounds = neomind_storage::AgentDefaults::get().max_rounds as usize;
        Self {
            max_rounds,
            recommended_tools: None,
            is_focused_plus: false,
        }
    }

    fn focused_plus(agent: &AiAgent) -> Self {
        let ceiling = neomind_storage::AgentDefaults::get().max_rounds as usize;
        Self {
            max_rounds: agent.max_chain_depth.clamp(1, ceiling),
            recommended_tools: Some(Self::build_focused_recommended_tools(agent)),
            is_focused_plus: true,
        }
    }

    fn build_focused_recommended_tools(agent: &AiAgent) -> Vec<String> {
        // Only list REAL tools here. `device` and `message` are not tools —
        // they are neomind CLI subcommands reached via `shell`. Naming them
        // made weak models call a non-existent `device(...)` on round 1 and
        // eat a NotFound + recovery round before the real work starts.
        let mut tools = vec!["shell".to_string(), "skill".to_string()];
        for r in &agent.resources {
            if matches!(r.resource_type, ResourceType::Command) {
                tools.push(format!("device:{}", r.resource_id));
            }
            if matches!(r.resource_type, ResourceType::ExtensionTool) {
                tools.push(r.resource_id.clone());
            }
        }
        tools
    }
}

// Sub-modules
mod analyzer;
mod command_executor;
mod compact;
mod context;
mod data_collector;
mod event_trigger;
mod intent;
mod llm_runtime;
mod memory;
mod response_parser;
pub(crate) mod stuck_detector;
mod tool_loop;
mod tool_prompt;
pub(crate) mod tool_result; // pub(crate): hallucinated_tool_hint reused by chat streaming path

// Re-export public types
pub(crate) use analyzer::AnalysisResult;
pub use context::{DataSourceRef, EventTriggerData};

// Re-export functions needed by sibling modules (via use super::*)
pub(crate) use context::{build_history_context, format_timestamp, truncate_to, HistoryConfig};
pub(crate) use data_collector::get_time_context;
pub(crate) use intent::extract_threshold;
pub(crate) use response_parser::{
    extract_command_from_description, extract_device_from_description, extract_json_from_codeblock,
    summarize_tool_output,
};

/// Resolve the role prompt for an agent.
/// Returns the agent's custom `system_prompt` if set, otherwise the default IoT role string.
pub(crate) fn resolve_role(agent: &neomind_storage::AiAgent, default: &str) -> String {
    agent
        .system_prompt
        .as_deref()
        .unwrap_or(default)
        .to_string()
}

/// Build JSON Schema parameters from extension command parameters.
fn build_parameters_schema(
    parameters: &[neomind_core::extension::ParameterDefinition],
) -> serde_json::Value {
    use neomind_core::extension::MetricDataType;
    use std::collections::HashMap;

    let mut properties = HashMap::new();
    let mut required = Vec::new();

    for param in parameters {
        let param_type = match param.param_type {
            MetricDataType::Float => "number",
            MetricDataType::Integer => "integer",
            MetricDataType::Boolean => "boolean",
            MetricDataType::String | MetricDataType::Enum { .. } => "string",
            MetricDataType::Binary => "string",
        };

        let mut param_schema = serde_json::json!({
            "type": param_type,
            "description": param.description,
        });

        if let MetricDataType::Enum { options } = &param.param_type {
            param_schema["enum"] = serde_json::json!(options);
        }

        if let Some(default_val) = &param.default_value {
            param_schema["default"] = serde_json::json!(default_val);
        }

        properties.insert(param.name.clone(), param_schema);

        if param.required {
            required.push(param.name.clone());
        }
    }

    serde_json::json!({
        "type": "object",
        "properties": properties,
        "required": required,
    })
}

/// Configuration for agent executor.
#[derive(Clone)]
pub struct AgentExecutorConfig {
    /// Agent store
    pub store: Arc<AgentStore>,
    /// Time series storage for data collection
    pub time_series_storage: Option<Arc<neomind_storage::TimeSeriesStore>>,
    /// Device service for command execution
    pub device_service: Option<Arc<DeviceService>>,
    /// Event bus for event subscription
    pub event_bus: Option<Arc<EventBus>>,
    /// Message manager for sending notifications (replaces AlertManager)
    pub message_manager: Option<Arc<MessageManager>>,
    /// LLM runtime for intent analysis (default)
    pub llm_runtime: Option<Arc<dyn neomind_core::llm::backend::LlmRuntime + Send + Sync>>,
    /// LLM backend store for per-agent backend lookup
    pub llm_backend_store: Option<Arc<LlmBackendStore>>,
    /// Phase 3.3: Extension registry for dynamic tool loading
    pub extension_registry: Option<Arc<neomind_core::extension::registry::ExtensionRegistry>>,
    /// Tool registry for function calling mode
    pub tool_registry: Option<Arc<crate::toolkit::ToolRegistry>>,
    /// Memory store for extracting learned patterns
    pub memory_store: Option<Arc<MarkdownMemoryStore>>,
    /// Per-LLM-backend semaphores concurrency limiting (shared with scheduler)
    pub backend_semaphores: Option<crate::ai_agent::scheduler::BackendSemaphores>,
    /// Skill registry for querying operation guides
    pub skill_registry: Option<crate::skills::SharedSkillRegistry>,
}

/// Context for agent execution.
#[derive(Clone)]
pub struct ExecutionContext {
    /// Agent being executed
    pub agent: AiAgent,
    /// Trigger type (schedule, event, manual)
    pub trigger_type: String,
    /// Current event data (if event-triggered)
    pub event_data: Option<serde_json::Value>,
    /// LLM backend for decision making
    pub llm_backend: Option<LlmBackend>,
    /// Execution ID for event tracking
    pub execution_id: String,
    /// Invocation input from API/Rule/Agent caller
    pub invocation_input: Option<super::AgentInput>,
    /// Optional per-execution cancellation token.
    ///
    /// Currently informational — populated for paths that own their own
    /// `ToolRegistry` (e.g. single-session chat). NOT installed on the
    /// shared scheduled-execution registry because concurrent agent
    /// executions would race on the slot. Scheduled executions are
    /// cancelled via `JoinHandle::abort()` at the scheduler level
    /// (see `AgentScheduler::stop`); the future-drop cascade triggers
    /// `PidKillGuard` for shell subprocesses and reqwest abort for
    /// in-flight LLM HTTP calls.
    pub cancellation_token: Option<tokio_util::sync::CancellationToken>,
}

/// AI Agent executor - handles execution of user-defined agents.
pub struct AgentExecutor {
    /// Agent store
    pub(crate) store: Arc<AgentStore>,
    /// Time series storage for data collection
    pub(crate) time_series_storage: Option<Arc<neomind_storage::TimeSeriesStore>>,
    /// Device service for command execution
    pub(crate) device_service: Option<Arc<DeviceService>>,
    /// Event bus for publishing events
    pub(crate) event_bus: Option<Arc<EventBus>>,
    /// Message manager for sending notifications (replaces AlertManager)
    pub(crate) message_manager: Option<Arc<MessageManager>>,
    /// Configuration
    pub(crate) _config: AgentExecutorConfig,
    /// LLM runtime (default)
    pub(crate) llm_runtime: Option<Arc<dyn neomind_core::llm::backend::LlmRuntime + Send + Sync>>,
    /// LLM backend store for per-agent backend lookup
    pub(crate) llm_backend_store: Option<Arc<LlmBackendStore>>,
    /// Event-triggered agents cache
    pub(crate) event_agents: Arc<RwLock<HashMap<String, AiAgent>>>,
    /// Track recent executions to prevent duplicates (agent_id, device_id -> timestamp)
    /// Deduplicates by device only, not by individual metrics
    pub(crate) recent_executions: Arc<RwLock<HashMap<String, i64>>>,
    /// LLM runtime cache: backend_id -> runtime
    /// Key format: "{backend_type}:{endpoint}:{model}" for cache invalidation
    pub(crate) llm_runtime_cache:
        Arc<RwLock<HashMap<String, Arc<dyn neomind_core::llm::backend::LlmRuntime + Send + Sync>>>>,
    /// Phase 3.3: Extension registry for dynamic tool loading
    pub(crate) extension_registry:
        Option<Arc<neomind_core::extension::registry::ExtensionRegistry>>,
    /// Tool registry for function calling mode (wrapped for late initialization)
    pub(crate) tool_registry: parking_lot::RwLock<Option<Arc<crate::toolkit::ToolRegistry>>>,
    /// Memory store for extracting learned patterns
    pub(crate) memory_store: Option<Arc<MarkdownMemoryStore>>,
    /// Per-LLM-backend semaphores for concurrency limiting (shared with scheduler)
    pub(crate) backend_semaphores: Option<crate::ai_agent::scheduler::BackendSemaphores>,
    /// Semaphore limiting concurrent tool executions (default: 6)
    pub(crate) tool_concurrency: Arc<Semaphore>,
}

/// Parse the LLM's final text response to extract situation_analysis, conclusion, and confidence.
///
/// Expects a JSON block like: ```json\n{"situation_analysis":"...","conclusion":"...","confidence":0.8}\n```
/// Falls back to sensible defaults if parsing fails.
impl AgentExecutor {
    /// Publish an event to the event bus (no-op if no bus is configured).
    async fn publish_event(&self, event: NeoMindEvent) {
        if let Some(ref bus) = self.event_bus {
            let _ = bus.publish(event).await;
        }
    }

    /// Create a new agent executor.
    pub async fn new(config: AgentExecutorConfig) -> AgentResult<Self> {
        let llm_runtime = config.llm_runtime.clone();
        let llm_backend_store = config.llm_backend_store.clone();
        let message_manager = config.message_manager.clone();
        let extension_registry = config.extension_registry.clone();
        Ok(Self {
            store: config.store.clone(),
            time_series_storage: config.time_series_storage.clone(),
            device_service: config.device_service.clone(),
            event_bus: config.event_bus.clone(),
            message_manager,
            _config: config.clone(),
            llm_runtime,
            llm_backend_store,
            event_agents: Arc::new(RwLock::new(HashMap::new())),
            recent_executions: Arc::new(RwLock::new(HashMap::new())),
            llm_runtime_cache: Arc::new(RwLock::new(HashMap::new())),
            extension_registry,
            tool_registry: parking_lot::RwLock::new(config.tool_registry.clone()),
            memory_store: config.memory_store.clone(),
            backend_semaphores: config.backend_semaphores.clone(),
            tool_concurrency: Arc::new(Semaphore::new(
                neomind_storage::AgentDefaults::get().tool_concurrency,
            )),
        })
    }

    /// Set the LLM runtime for intent parsing.
    pub async fn set_llm_runtime(
        &mut self,
        llm: Arc<dyn neomind_core::llm::backend::LlmRuntime + Send + Sync>,
    ) {
        self.llm_runtime = Some(llm);
    }

    /// Check whether tool mode should be used for this agent execution.
    ///
    /// All agents use tool-calling when the LLM and tool registry support it.
    /// Falls back to structured JSON analysis only when tool-calling is unavailable.
    fn should_use_tools(
        &self,
        agent: &AiAgent,
        llm_runtime: &Arc<dyn LlmRuntime + Send + Sync>,
    ) -> bool {
        let llm_supports_tools = llm_runtime.capabilities().function_calling;
        let registry_available = self.tool_registry.read().is_some();

        if !(llm_supports_tools && registry_available) {
            tracing::info!(
                agent_id = %agent.id,
                llm_supports_tools,
                registry_available,
                "Tool mode NOT activated - falling back to structured analysis"
            );
            return false;
        }

        true
    }

    /// Execute agent using tool/function-calling mode.
    ///
    /// In this mode, the LLM receives tool definitions and can make tool calls
    /// that are parsed from its text response, executed, and the results fed back
    /// for further reasoning.
    /// Filter tool definitions based on agent's allowed_tools config.
    ///
    /// Uses cached definitions directly — no JSON round-trip.
    /// Sanitizes tool names for OpenAI-compatible API compatibility
    /// (replacing `.` and `:` with `_`) and returns a reverse mapping
    /// so tool calls from the LLM can be routed back to the correct tool.
    fn filter_tools(
        registry: &crate::toolkit::registry::ToolRegistry,
        tool_config: &Option<AgentToolConfig>,
    ) -> (
        Vec<neomind_core::llm::backend::ToolDefinition>,
        std::collections::HashMap<String, String>,
    ) {
        use neomind_core::llm::backend::sanitize_tool_name;

        let defs = registry.definitions_for_llm();
        let mut name_map: std::collections::HashMap<String, String> =
            std::collections::HashMap::new();

        let to_tool_def = |d: &crate::toolkit::tool::ToolDefinition,
                           map: &mut std::collections::HashMap<String, String>|
         -> neomind_core::llm::backend::ToolDefinition {
            let sanitized = sanitize_tool_name(&d.name);
            if sanitized != d.name {
                map.entry(sanitized.clone())
                    .or_insert_with(|| d.name.clone());
            }
            neomind_core::llm::backend::ToolDefinition {
                name: sanitized,
                description: d.description.clone(),
                parameters: d.parameters.clone(),
            }
        };

        // Both Focused and Free get the same tool set.
        // Focused JSON path: execute_decisions whitelist enforces scope.
        // Free mode (tool calling): all tools available, agent decides what to use.
        // Tool mode explicitly disabled → no tools at all (text-only responses).
        if let Some(config) = tool_config {
            if !config.enabled {
                return (Vec::new(), std::collections::HashMap::new());
            }
        }
        let filtered = match tool_config {
            Some(config) if !config.allowed_tools.is_empty() => {
                let allowed: std::collections::HashSet<&str> =
                    config.allowed_tools.iter().map(|s| s.as_str()).collect();

                defs.iter()
                    .filter(|d| allowed.contains(d.name.as_str()))
                    .map(|d| to_tool_def(d, &mut name_map))
                    .collect()
            }
            _ => defs.iter().map(|d| to_tool_def(d, &mut name_map)).collect(),
        };

        (filtered, name_map)
    }

    // run_tool_loop and helpers are now in tool_loop.rs

    // process_tool_results, generate_phase2_summary, tool_name_to_semantic_type,
    // and build_tool_result are now in tool_result.rs

    async fn execute_with_tools(
        &self,
        agent: &AiAgent,
        data_collected: &[DataCollected],
        llm_runtime: Arc<dyn LlmRuntime + Send + Sync>,
        execution_id: &str,
        invocation_input: Option<&super::AgentInput>,
    ) -> AgentResult<(DecisionProcess, neomind_storage::ExecutionResult)> {
        let registry = self
            .tool_registry
            .read()
            .clone()
            .ok_or_else(|| NeoMindError::Tool("Tool registry not available".to_string()))?;

        // NOTE: per-execution MemoryTool handles are prepared by the scheduler
        // (execute_scheduled / run_manual) before calling execute_internal → this method.
        // Do NOT call prepare_memory_tool_execution here — that would create a SECOND set
        // of Arc handles, and the memory tool would register new files into the wrong handle.

        // Build mode-specific config
        let tool_config = match agent.execution_mode {
            neomind_storage::agents::ExecutionMode::Free => ToolLoopConfig::free(),
            neomind_storage::agents::ExecutionMode::Focused => ToolLoopConfig::focused_plus(agent),
        };

        let (mut filtered_tools, mut tool_name_map) =
            Self::filter_tools(&registry, &agent.tool_config);

        // When the LLM already supports vision AND images are embedded in the
        // multimodal user message, remove the `vision` tool to prevent redundant
        // calls.  The LLM sees the images directly — calling a separate VLM
        // wastes a round, passes truncated data (the LLM can only reference a
        // tiny preview in tool arguments), and may hit rate limits.
        let has_images_in_data = data_collected.iter().any(|d| {
            d.values
                .get("_is_image")
                .and_then(|v| v.as_bool())
                .unwrap_or(false)
        });
        let llm_supports_vision = llm_runtime.capabilities().supports_images;
        if has_images_in_data && llm_supports_vision {
            let before = filtered_tools.len();
            let vision_names = ["vision"];
            filtered_tools.retain(|t| !vision_names.contains(&t.name.as_str()));
            tool_name_map.retain(|_, orig| !vision_names.contains(&orig.as_str()));
            let removed = before - filtered_tools.len();
            if removed > 0 {
                tracing::info!(
                    agent_id = %agent.id,
                    removed,
                    "Excluded vision tool(s) — images already visible to multimodal LLM"
                );
            }
        }

        if !tool_name_map.is_empty() {
            tracing::debug!(
                agent_id = %agent.id,
                tools = ?tool_name_map,
                "Sanitized tool names for API compatibility"
            );
        }

        // Pre-fetch knowledge file content for inline injection.
        // This avoids wasting a tool-call round to read files the agent already
        // knows about — especially valuable in Focused+ mode with only 3 rounds.
        let knowledge_content = self.prefetch_knowledge_files(
            &agent.id,
            &agent.memory.knowledge_files,
            agent.context_window_size,
        );

        let system_prompt = tool_prompt::build_tool_system_prompt(
            agent,
            data_collected,
            invocation_input,
            &tool_config,
            knowledge_content.as_ref(),
        );
        let mut messages = tool_prompt::build_tool_messages(&system_prompt, data_collected);

        // The bound device image is injected into the multimodal user message
        // above, so the LLM SEES it — but it cannot reproduce the base64 to
        // pass it to an extension tool (YOLO / grounding). Seed the per-execution
        // LargeDataCache with the real image so `resolve_cached_arguments`
        // auto-injects it into image-shaped tool args instead of the LLM's
        // truncated fragment (task #50: extensions returned `null`).
        let bound_image = tool_prompt::bound_image_data_url(data_collected);

        let mut loop_output = self
            .run_tool_loop(
                agent,
                &registry,
                &llm_runtime,
                &filtered_tools,
                &mut messages,
                execution_id,
                tool_config.max_rounds,
                &tool_name_map,
                bound_image.as_deref(),
            )
            .await;

        // Surface LLM failures as explicit errors.
        //
        // Three failure modes:
        // 1. `llm_generation_failed`: the LLM generate() call itself failed
        //    (HTTP 403, timeout, network). This is a definitive failure —
        //    the sentinel string is only set in tool_loop.rs when generate()
        //    returns Err. Surface regardless of whether some tools ran first.
        // 2. `has_malformed_output` + `no_tools_executed`: the LLM produced
        //    XML-like fragments that the parser couldn't recognize, and no
        //    tools were executed at all. Only trigger for this combination
        //    because malformed output after successful tools might be the
        //    LLM's legitimate final text containing XML.
        //    **Detection requires BOTH opening and closing tags** — orphan
        //    closing tags alone (e.g. qwen3.6-35b-a3b under complex multimodal
        //    context emits `</parameter></function></tool_call>` as content
        //    with no opening tags and no `tool_calls` API field) are model
        //    noise, not a real tool-call attempt. Stripped below.
        let no_tools_executed = loop_output.all_tool_results.is_empty();
        let classification = classify_tool_call_text(&loop_output.final_text);
        let has_malformed_output = classification.is_malformed;
        let llm_generation_failed =
            loop_output.final_text == "LLM generation failed during tool execution.";

        if llm_generation_failed || (no_tools_executed && has_malformed_output) {
            // If we have the real LLM error, surface it. Otherwise use a generic
            // message for the malformed-output path (no LLM error captured).
            let msg = match &loop_output.last_llm_error {
                Some(e) => format!("LLM tool-calling failed: {}", e),
                None if has_malformed_output => {
                    "LLM tool-calling produced malformed output".to_string()
                }
                _ => "LLM tool-calling failed".to_string(),
            };
            tracing::warn!(
                agent_id = %agent.id,
                malformed_output = has_malformed_output,
                has_llm_error = loop_output.last_llm_error.is_some(),
                tools_executed = !no_tools_executed,
                "Tool-calling failed — propagating error"
            );
            return Err(NeoMindError::Llm(msg));
        }

        // Orphan closing-tag cleanup: small thinking-capable models
        // (e.g. qwen3.6-35b-a3b MoE) sometimes emit bare closing tags
        // (`</parameter></function></tool_call>`) as content under complex
        // multimodal + multi-round context, with NO corresponding opening
        // tags and NO native `tool_calls` API field. This is model noise, not
        // a real tool-call attempt — strip the orphan tags so downstream
        // result building sees clean text and treats this as a normal stop.
        if classification.has_orphan_closing_tags {
            tracing::warn!(
                agent_id = %agent.id,
                orphan_tags_detected = true,
                stripped_preview = %loop_output
                    .final_text
                    .chars()
                    .take(200)
                    .collect::<String>(),
                "Stripping orphan XML closing tags from LLM output (model noise)"
            );
            loop_output.final_text = classification.cleaned_text;
        }

        let (decision_process, execution_result) =
            tool_result::build_tool_result(agent, data_collected, loop_output);

        Ok((decision_process, execution_result))
    }

    /// Get the agent store.
    pub fn store(&self) -> Arc<AgentStore> {
        self.store.clone()
    }

    /// Update the tool registry (e.g. after extensions are loaded).
    pub fn set_tool_registry(&self, registry: Arc<crate::toolkit::ToolRegistry>) {
        *self.tool_registry.write() = Some(registry);
    }

    // ========================================================================
    // Extension Tools Integration
    // ========================================================================

    /// Get the extension registry.
    pub fn extension_registry(
        &self,
    ) -> Option<Arc<neomind_core::extension::registry::ExtensionRegistry>> {
        self.extension_registry.clone()
    }

    /// Get available tools from extensions.
    ///
    /// Returns a list of tool definitions from all registered extensions.
    /// Each extension command becomes a tool that AI agents can call.
    pub async fn get_extension_tools(&self) -> Vec<serde_json::Value> {
        let mut result = Vec::new();

        if let Some(ref registry) = self.extension_registry {
            let extensions = registry.list().await;

            for info in extensions {
                let metadata_id = info.metadata.id.clone();
                let commands = info.commands;

                // Convert each command to a tool description
                for cmd in commands {
                    // Build parameters JSON schema from command parameters
                    let parameters = build_parameters_schema(&cmd.parameters);

                    result.push(serde_json::json!({
                        "name": format!("{}_{}", metadata_id, cmd.name),
                        "description": cmd.description,
                        "parameters": parameters,
                        "extension_id": metadata_id,
                        "command": cmd.name,
                    }));
                }
            }
        }

        result
    }

    /// Execute an extension command.
    ///
    /// This allows agents to call tools provided by extensions.
    pub async fn execute_extension_command(
        &self,
        extension_id: &str,
        command: &str,
        args: &serde_json::Value,
    ) -> AgentResult<serde_json::Value> {
        let registry = self
            .extension_registry
            .as_ref()
            .ok_or_else(|| NeoMindError::Config("Extension registry not configured".to_string()))?;

        registry
            .execute_command(extension_id, command, args)
            .await
            .map_err(|e| NeoMindError::Tool(e.to_string()))
    }

    /// Get all extension tools.
    ///
    /// This is a convenience method that returns all extension tools.
    pub async fn get_all_extension_tools(&self) -> Vec<serde_json::Value> {
        self.get_extension_tools().await
    }

    /// Send a progress event for an agent execution.
    async fn send_progress(
        &self,
        agent_id: &str,
        execution_id: &str,
        stage: &str,
        stage_label: &str,
        details: Option<&str>,
    ) {
        self.publish_event(neomind_core::NeoMindEvent::AgentProgress {
            agent_id: agent_id.to_string(),
            execution_id: execution_id.to_string(),
            stage: stage.to_string(),
            stage_label: stage_label.to_string(),
            progress: None,
            details: details.map(|d| d.to_string()),
            timestamp: chrono::Utc::now().timestamp(),
        })
        .await;
    }

    /// Send a thinking event for an agent execution.
    async fn send_thinking(
        &self,
        agent_id: &str,
        execution_id: &str,
        step_number: u32,
        description: &str,
    ) {
        self.publish_event(neomind_core::NeoMindEvent::AgentThinking {
            agent_id: agent_id.to_string(),
            execution_id: execution_id.to_string(),
            step_number,
            step_type: "progress".to_string(),
            description: description.to_string(),
            details: None,
            timestamp: chrono::Utc::now().timestamp(),
        })
        .await;
    }

    /// Execute an agent and record the full decision process.
    pub async fn execute_agent(
        &self,
        agent: AiAgent,
        event_data: Option<EventTriggerData>,
        invocation_input: Option<super::AgentInput>,
    ) -> AgentResult<AgentExecutionRecord> {
        let agent_id = agent.id.clone();
        let agent_name = agent.name.clone();
        let execution_id = uuid::Uuid::new_v4().to_string();
        let start_time = std::time::Instant::now();
        let timestamp = chrono::Utc::now().timestamp();

        // Update agent status to executing
        self.store
            .update_agent_status(&agent_id, neomind_storage::AgentStatus::Executing, None)
            .await
            .map_err(|e| NeoMindError::Storage(format!("Failed to update status: {}", e)))?;

        // RAII guard: if execute_agent is cancelled/panics before reaching the
        // normal status-reset at the end, this guard will reset the status on drop.
        // Without this, an agent can be permanently stuck in Executing state.
        struct StatusGuard {
            store: Arc<AgentStore>,
            agent_id: String,
            armed: std::cell::Cell<bool>,
        }

        impl Drop for StatusGuard {
            fn drop(&mut self) {
                if self.armed.get() {
                    let store = self.store.clone();
                    let aid = self.agent_id.clone();
                    tokio::spawn(async move {
                        tracing::warn!(
                            agent_id = %aid,
                            "execute_agent dropped before status reset — force-resetting to Active"
                        );
                        let _ = store
                            .update_agent_status(
                                &aid,
                                neomind_storage::AgentStatus::Active,
                                Some("Execution interrupted - force reset".to_string()),
                            )
                            .await;
                    });
                }
            }
        }

        let status_guard = StatusGuard {
            store: self.store.clone(),
            agent_id: agent_id.clone(),
            armed: std::cell::Cell::new(true),
        };

        // Determine trigger type and context event_data based on whether we have event trigger
        let trigger_type = match &event_data {
            Some(ed) => format!("event:{}", ed.source.field),
            None => "manual".to_string(),
        };

        let context_event_data = event_data.as_ref().map(|ed| {
            serde_json::json!({
                "source_type": ed.source.source_type,
                "source_id": ed.source.source_id,
                "field": ed.source.field,
                "value": serde_json::to_value(&ed.value).unwrap_or_default(),
                "timestamp": ed.timestamp,
            })
        });

        // Create execution context
        let context = ExecutionContext {
            agent: agent.clone(),
            trigger_type: trigger_type.clone(),
            event_data: context_event_data,
            llm_backend: None,
            execution_id: execution_id.clone(),
            invocation_input: invocation_input.clone(),
            cancellation_token: None,
        };

        // Emit agent execution started event
        tracing::debug!(
            agent_id = %agent_id,
            execution_id = %execution_id,
            trigger_type = %trigger_type,
            has_event_bus = self.event_bus.is_some(),
            "Emitting AgentExecutionStarted event"
        );
        self.publish_event(NeoMindEvent::AgentExecutionStarted {
            agent_id: agent_id.clone(),
            agent_name: agent_name.clone(),
            execution_id: execution_id.clone(),
            trigger_type: trigger_type.clone(),
            timestamp,
        })
        .await;

        // Inject per-execution agent context into MemoryTool (concurrency-safe).
        let per_exec_knowledge_files = self
            .tool_registry
            .read()
            .as_ref()
            .and_then(|registry| {
                registry.prepare_memory_tool_execution(
                    agent_id.clone(),
                    agent.memory.knowledge_files.clone(),
                )
            })
            .map(|h| h.1);

        // Execute with error handling for stability.
        // Wrap with catch_unwind so panics (e.g. UTF-8 slice bugs) are converted
        // to Err and a proper Failed execution record is created instead of
        // silently disappearing.
        // Also enforce a global timeout (5 min) as a safety net against runaway
        // execution (e.g., 30 rounds × slow extension tools).
        let global_execution_timeout_secs =
            neomind_storage::AgentDefaults::get().execution_timeout_secs;
        let execution_result: AgentResult<(DecisionProcess, StorageExecutionResult)> =
            match tokio::time::timeout(
                std::time::Duration::from_secs(global_execution_timeout_secs),
                std::panic::AssertUnwindSafe(self.execute_internal(
                    context,
                    event_data.clone(),
                    per_exec_knowledge_files.clone(),
                ))
                .catch_unwind(),
            )
            .await
            {
                Ok(Ok(Ok(result))) => Ok(result),
                Ok(Ok(Err(e))) => Err(e),
                Ok(Err(panic_payload)) => {
                    let msg = if let Some(s) = panic_payload.downcast_ref::<&str>() {
                        (*s).to_string()
                    } else if let Some(s) = panic_payload.downcast_ref::<String>() {
                        (*s).clone()
                    } else {
                        "Agent execution panicked".to_string()
                    };
                    tracing::error!(
                        agent_id = %agent_id,
                        execution_id = %execution_id,
                        error = %msg,
                        "Agent execution panicked — converting to error"
                    );
                    Err(NeoMindError::Llm(format!("Execution panic: {}", msg)))
                }
                Err(_) => {
                    tracing::error!(
                        agent_id = %agent_id,
                        execution_id = %execution_id,
                        timeout_secs = global_execution_timeout_secs,
                        "Agent execution timed out globally"
                    );
                    Err(NeoMindError::Llm(format!(
                        "Execution timed out after {}s",
                        global_execution_timeout_secs
                    )))
                }
            };

        let duration_ms = start_time.elapsed().as_millis() as u64;

        // Build execution record
        let record = match execution_result {
            Ok((decision_process, result)) => {
                // Update stats
                if let Err(e) = self
                    .store
                    .update_agent_stats(&agent_id, true, duration_ms)
                    .await
                {
                    tracing::error!(
                        agent_id = %agent_id,
                        error = %e,
                        "Failed to update agent stats after successful execution"
                    );
                }

                AgentExecutionRecord {
                    id: execution_id.clone(),
                    agent_id: agent_id.clone(),
                    timestamp,
                    trigger_type: trigger_type.clone(),
                    status: ExecutionStatus::Completed,
                    decision_process,
                    result: Some(result),
                    duration_ms,
                    error: None,
                }
            }
            Err(e) => {
                // Update stats with failure
                if let Err(stats_err) = self
                    .store
                    .update_agent_stats(&agent_id, false, duration_ms)
                    .await
                {
                    tracing::error!(
                        agent_id = %agent_id,
                        error = %stats_err,
                        "Failed to update agent stats after failed execution"
                    );
                }

                // Write failure entry to journal so next execution can see the
                // failure pattern and potentially adapt its behavior.
                if let Ok(Some(agent_data)) = self.store.get_agent(&agent_id).await {
                    let mut memory = agent_data.memory;
                    let error_msg = truncate_to(&e.to_string(), 300);
                    memory.journal.records.push(ExecutionRecord {
                        timestamp: chrono::Utc::now().timestamp(),
                        execution_id: execution_id.clone(),
                        outcome: error_msg,
                        action_taken: "execution failed".to_string(),
                        success: false,
                        stop_reason: String::new(),
                    });
                    // FIFO — keep only max_records
                    while memory.journal.records.len() > memory.journal.max_records {
                        memory.journal.records.remove(0);
                    }
                    memory.updated_at = chrono::Utc::now().timestamp();
                    if let Err(mem_err) = self.store.update_agent_memory(&agent_id, memory).await {
                        tracing::warn!(
                            agent_id = %agent_id,
                            error = %mem_err,
                            "Failed to update agent memory with failure entry"
                        );
                    }
                }

                AgentExecutionRecord {
                    id: execution_id.clone(),
                    agent_id: agent_id.clone(),
                    timestamp,
                    trigger_type: trigger_type.clone(),
                    status: ExecutionStatus::Failed,
                    decision_process: DecisionProcess {
                        situation_analysis: format!("Execution failed: {}", e),
                        data_collected: vec![],
                        reasoning_steps: vec![],
                        decisions: vec![],
                        conclusion: format!("Failed: {}", e),
                        confidence: 0.0,
                        stop_reason: String::new(),
                    },
                    result: None,
                    duration_ms,
                    error: Some(e.to_string()),
                }
            }
        };

        // Save execution record and conversation turn in a single transaction
        self.store
            .save_execution_with_conversation(&record, Some(&agent_id), None)
            .await
            .map_err(|e| NeoMindError::Storage(format!("Failed to save execution: {}", e)))?;

        tracing::debug!(
            agent_id = %agent_id,
            execution_id = %execution_id,
            "Execution and conversation turn saved successfully"
        );

        // Reset agent status based on result.
        //
        // For *transient* failures (network, timeout, rate-limit, the global
        // 300s timeout), keep the agent Active so the scheduler will retry it
        // on the next trigger. Only *permanent* failures (malformed output,
        // auth errors, model not found) demote the agent to Error, requiring
        // manual intervention to re-enable.
        //
        // Disarm the RAII guard — normal completion handles status reset
        status_guard.armed.set(false);

        let new_status = if record.status == ExecutionStatus::Completed {
            neomind_storage::AgentStatus::Active
        } else if is_transient_failure(record.error.as_deref()) {
            tracing::info!(
                agent_id = %agent_id,
                execution_id = %execution_id,
                error = ?record.error,
                "Transient execution failure — keeping agent Active for retry"
            );
            neomind_storage::AgentStatus::Active
        } else {
            neomind_storage::AgentStatus::Error
        };

        // Retry status reset once on failure to prevent agent getting stuck in Executing state
        match self
            .store
            .update_agent_status(&agent_id, new_status, record.error.clone())
            .await
        {
            Ok(()) => {}
            Err(e) => {
                tracing::error!(
                    agent_id = %agent_id,
                    new_status = ?new_status,
                    error = %e,
                    "Failed to reset agent status after execution, retrying once"
                );
                // Single retry after a short delay
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                if let Err(retry_err) = self
                    .store
                    .update_agent_status(&agent_id, new_status, record.error.clone())
                    .await
                {
                    tracing::error!(
                        agent_id = %agent_id,
                        error = %retry_err,
                        "Agent may be stuck in Executing status after retry failed"
                    );
                }
            }
        }

        // Emit agent execution completed event
        let completion_timestamp = chrono::Utc::now().timestamp();
        self.publish_event(NeoMindEvent::AgentExecutionCompleted {
            agent_id: agent_id.clone(),
            execution_id: execution_id.clone(),
            success: record.status == ExecutionStatus::Completed,
            duration_ms: record.duration_ms,
            error: record.error.clone(),
            timestamp: completion_timestamp,
        })
        .await;

        tracing::info!(
            agent_id = %agent_id,
            agent_name = %agent_name,
            execution_id = %execution_id,
            status = ?record.status,
            duration_ms = record.duration_ms,
            "Agent execution completed"
        );

        Ok(record)
    }

    /// Execute multiple agents in parallel for improved performance.
    ///
    /// This is especially useful for multi-agent scenarios where agents
    /// have independent tasks and can run concurrently.
    ///
    /// # Example
    /// ```text
    /// let agents = vec![monitor_agent, executor_agent, analyst_agent];
    /// let results = executor.execute_agents_parallel(agents).await?;
    /// // Results are returned in the same order as input agents
    /// ```
    pub async fn execute_agents_parallel(
        &self,
        agents: Vec<AiAgent>,
    ) -> AgentResult<Vec<AgentExecutionRecord>> {
        use futures::future::join_all;

        // Sort agents by priority (higher priority first)
        let mut sorted_agents = agents;
        sorted_agents.sort_by(|a, b| b.priority.cmp(&a.priority));

        let executor_ref = self;
        let futures: Vec<_> = sorted_agents
            .into_iter()
            .map(|agent| executor_ref.execute_agent(agent, None, None))
            .collect();

        let results = join_all(futures).await;

        // Collect results, converting any errors into a combined error
        let mut records = Vec::new();
        let mut errors = Vec::new();

        for result in results {
            match result {
                Ok(record) => records.push(record),
                Err(e) => errors.push(e),
            }
        }

        if !errors.is_empty() {
            tracing::warn!(
                count = errors.len(),
                "Some agents failed during parallel execution"
            );
        }

        if records.is_empty() && !errors.is_empty() {
            return Err(NeoMindError::Storage(format!(
                "All {} agents failed. First error: {}",
                errors.len(),
                errors[0]
            )));
        }

        Ok(records)
    }

    /// Internal execution logic.
    async fn execute_internal(
        &self,
        context: ExecutionContext,
        event_data: Option<EventTriggerData>,
        per_exec_knowledge_files: Option<
            Arc<tokio::sync::RwLock<Vec<neomind_storage::KnowledgeFileRef>>>,
        >,
    ) -> AgentResult<(DecisionProcess, StorageExecutionResult)> {
        let mut agent = context.agent;
        let agent_id = agent.id.clone();
        let execution_id = context.execution_id.clone();
        let mut step_num = 1u32;

        // Progress: Collecting data
        self.send_progress(
            &agent_id,
            &execution_id,
            "collecting",
            "Collecting data",
            Some("Gathering sensor data..."),
        )
        .await;

        // Step 1: Collect data (with or without event data)
        let data_collected = if let Some(ref ed) = event_data {
            self.collect_data_with_event(&agent, ed).await?
        } else {
            self.collect_data(&agent).await?
        };

        // If event-triggered collection returned no usable data (e.g. image
        // extraction failed due to malformed field names), skip LLM analysis
        // and return a minimal result instead of producing a meaningless empty
        // execution record.
        if event_data.is_some() && data_collected.is_empty() {
            tracing::warn!(
                agent_id = %agent_id,
                "Skipping analysis — event data collection produced no usable data"
            );
            self.send_progress(
                &agent_id,
                &execution_id,
                "skipped",
                "Skipped",
                Some("No usable data collected from event"),
            )
            .await;

            let decision_process = DecisionProcess {
                situation_analysis: "Event data collection failed — no usable data to analyze.".to_string(),
                data_collected: vec![],
                reasoning_steps: vec![],
                decisions: vec![],
                conclusion: "Execution skipped: event data was recognized as an image metric but image extraction failed. Check device data format and field names.".to_string(),
                confidence: 0.0,
                stop_reason: String::new(),
            };
            let exec_result = neomind_storage::ExecutionResult {
                actions_executed: vec![],
                report: None,
                notifications_sent: vec![],
                summary: "Skipped: event data extraction failed".to_string(),
                success_rate: 0.0,
            };

            // Write a `success: false` journal entry so this skip is visible
            // to future executions and to monitoring. Without this, the
            // failure mode is invisible: outer Ok branch only updates stats
            // (counted as success), and no journal entry is created — the
            // agent can never "learn" that its bound event source is broken.
            if let Err(mem_err) = self
                .update_memory(
                    &agent,
                    &[],
                    "Event skipped: no usable data collected from event trigger",
                    &execution_id,
                    false,
                    "",
                )
                .await
            {
                tracing::warn!(
                    agent_id = %agent_id,
                    error = %mem_err,
                    "Failed to write skip journal entry for empty event data"
                );
            }

            return Ok((decision_process, exec_result));
        }

        // Send thinking events for each data source collected
        for data in &data_collected {
            let desc = if event_data.is_some() {
                format!("Collecting {}: {} data points", data.source, data.data_type)
            } else {
                format!("Collected data source: {}", data.source)
            };
            self.send_thinking(&agent_id, &execution_id, step_num, &desc)
                .await;
            step_num += 1;
            // Small delay for visual effect
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }

        // Progress: Analyzing
        self.send_progress(
            &agent_id,
            &execution_id,
            "analyzing",
            "Analyzing",
            Some(&format!(
                "Analyzing {} data points...",
                data_collected.len()
            )),
        )
        .await;

        // Step 1.5: Parse intent if not already done
        let parsed_intent = if agent.parsed_intent.is_none() {
            match self.parse_intent(&agent.user_prompt).await {
                Ok(intent) => {
                    // Update agent with parsed intent
                    if let Err(e) = self
                        .store
                        .update_agent_parsed_intent(&agent.id, Some(intent.clone()))
                        .await
                    {
                        tracing::warn!(agent_id = %agent.id, error = %e, "Failed to store parsed intent");
                    }
                    Some(intent)
                }
                Err(e) => {
                    tracing::warn!(agent_id = %agent.id, error = %e, "Failed to parse intent, using default");
                    None
                }
            }
        } else {
            agent.parsed_intent.clone()
        };

        // Update agent reference with parsed intent
        if let Some(ref intent) = parsed_intent {
            agent.parsed_intent = Some(intent.clone());
        }

        // Step 2: Analyze situation — returns AnalysisResult which branches
        // Free vs Focused.
        let analysis = self
            .analyze_situation_with_intent(
                &agent,
                &data_collected,
                parsed_intent.as_ref(),
                &context.execution_id,
                context.invocation_input.as_ref(),
            )
            .await?;

        match analysis {
            // ── Tool-calling path (Free + Focused+) ────────────────────────
            // Tool-calling mode already produced a full DecisionProcess and
            // ExecutionResult.  We only need to update memory and return.
            AnalysisResult::Free {
                decision_process,
                execution_result,
            } => {
                self.send_thinking(
                    &agent_id,
                    &execution_id,
                    step_num,
                    &format!(
                        "Tool-calling analysis completed: {} tool call(s), confidence {:.0}%",
                        decision_process.decisions.len(),
                        decision_process.confidence * 100.0
                    ),
                )
                .await;

                // Update memory with Free mode results
                // Reflect partial failures: mark as failed when success_rate < 1.0
                let overall_success = execution_result.success_rate >= 1.0;
                let mut updated_memory = self
                    .update_memory(
                        &agent,
                        &decision_process.decisions,
                        &decision_process.conclusion,
                        &execution_id,
                        overall_success,
                        &decision_process.stop_reason,
                    )
                    .await?;

                // Sync knowledge_files from per-execution MemoryTool handle
                if let Some(ref handle) = per_exec_knowledge_files {
                    updated_memory.knowledge_files = handle.read().await.clone();
                }

                // Auto-init knowledge file on first SUCCESSFUL execution
                self.auto_init_knowledge_file(
                    &agent,
                    &mut updated_memory,
                    &decision_process.conclusion,
                    overall_success,
                );

                // Cap knowledge_files FIFO. The MemoryTool can append
                // arbitrary new files; without a cap a runaway agent
                // (or a long-lived one accumulating one file per execution)
                // bloats both storage and the system prompt —
                // `prefetch_knowledge_files` injects ALL file contents
                // into context. Same trim pattern as `journal.records`
                // (memory.rs:49-51) and `user_messages` (storage
                // MAX_USER_MESSAGES=50).
                while updated_memory.knowledge_files.len() > memory::MAX_KNOWLEDGE_FILES {
                    let dropped = updated_memory.knowledge_files.remove(0);
                    tracing::info!(
                        agent_id = %agent.id,
                        dropped_file = %dropped.name,
                        remaining = updated_memory.knowledge_files.len(),
                        "Trimmed knowledge file exceeding MAX_KNOWLEDGE_FILES (FIFO)"
                    );
                }

                self.store
                    .update_agent_memory(&agent.id, updated_memory.clone())
                    .await
                    .map_err(|e| {
                        NeoMindError::Storage(format!("Failed to update memory: {}", e))
                    })?;

                // Extract learned patterns into system memory
                // DISABLED: per-execution memory extraction was turned off (token
                // cost, wrong model). The memory scheduler (memory/scheduler.rs)
                // does NOT compensate — it only does temp-file cleanup. So agents
                // currently learn only via explicit memory-tool calls. Proper
                // extraction is tracked as follow-up work (Mem0 single-pass).

                tracing::debug!(
                    agent_id = %agent_id,
                    "[TOOL-CALLING] Returning direct results — skipped Focused JSON post-processing"
                );

                Ok((decision_process, execution_result))
            }

            // ── Focused path ───────────────────────────────────────────────
            // Standard single-pass LLM or rule-based analysis.  Follow the
            // original pipeline: execute_decisions → report → memory → store.
            AnalysisResult::Focused {
                situation_analysis,
                reasoning_steps,
                decisions,
                conclusion,
            } => {
                // Send thinking event for analysis completion
                self.send_thinking(
                    &agent_id,
                    &execution_id,
                    step_num,
                    &format!(
                        "Analysis completed: Generated {} decision(s)",
                        decisions.len()
                    ),
                )
                .await;
                step_num += 1;

                // Progress: Executing decisions
                self.send_progress(
                    &agent_id,
                    &execution_id,
                    "executing",
                    "Executing decisions",
                    Some(&format!("Executing {} decision(s)...", decisions.len())),
                )
                .await;

                // Send initial executing status
                self.send_thinking(
                    &agent_id,
                    &execution_id,
                    step_num,
                    &format!("Starting execution of {} decision(s)", decisions.len()),
                )
                .await;
                step_num += 1;

                // Step 3: Execute decisions
                let (actions_executed, notifications_sent) =
                    self.execute_decisions(&agent, &decisions).await?;

                // Send thinking events for each action executed
                for action in &actions_executed {
                    self.send_thinking(
                        &agent_id,
                        &execution_id,
                        step_num,
                        &format!("Executing: {} -> {}", action.action_type, action.target),
                    )
                    .await;
                    step_num += 1;
                    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                }

                // Send thinking events for notifications
                for notification in &notifications_sent {
                    self.send_thinking(
                        &agent_id,
                        &execution_id,
                        step_num,
                        &format!("Sending notification: {}", notification.message),
                    )
                    .await;
                    step_num += 1;
                    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                }

                // Send completion event for executing stage
                if actions_executed.is_empty() && notifications_sent.is_empty() {
                    self.send_thinking(
                        &agent_id,
                        &execution_id,
                        step_num,
                        "Execution completed: No additional actions required",
                    )
                    .await;
                } else {
                    self.send_thinking(
                        &agent_id,
                        &execution_id,
                        step_num,
                        &format!(
                            "Execution completed: {} action(s), {} notification(s)",
                            actions_executed.len(),
                            notifications_sent.len()
                        ),
                    )
                    .await;
                }

                // Note: refine_conclusion_with_query_results is intentionally NOT called
                // here. This Focused JSON path does not produce data_query actions
                // (those come from tool-calling mode). Calling it would always return
                // None and waste an await.

                // Step 4: Generate report if needed
                let report = self.maybe_generate_report(&agent, &data_collected).await?;

                // Step 5: Update memory with learnings
                // Reflect partial failures in action execution
                let focused_success =
                    actions_executed.is_empty() || actions_executed.iter().all(|a| a.success);
                let mut updated_memory = self
                    .update_memory(
                        &agent,
                        &decisions,
                        &conclusion,
                        &execution_id,
                        focused_success,
                        "",
                    )
                    .await?;

                // Sync knowledge_files from per-execution MemoryTool handle
                if let Some(ref handle) = per_exec_knowledge_files {
                    updated_memory.knowledge_files = handle.read().await.clone();
                }

                // Auto-init knowledge file on first SUCCESSFUL execution
                self.auto_init_knowledge_file(
                    &agent,
                    &mut updated_memory,
                    &conclusion,
                    focused_success,
                );

                // Save updated memory
                self.store
                    .update_agent_memory(&agent.id, updated_memory.clone())
                    .await
                    .map_err(|e| {
                        NeoMindError::Storage(format!("Failed to update memory: {}", e))
                    })?;

                // Bridge: extract learned patterns into system memory
                // DISABLED: per-execution memory extraction was turned off (token
                // cost, wrong model). The memory scheduler (memory/scheduler.rs)
                // does NOT compensate — it only does temp-file cleanup. So agents
                // currently learn only via explicit memory-tool calls. Proper
                // extraction is tracked as follow-up work (Mem0 single-pass).

                // Calculate confidence from reasoning
                let confidence = if reasoning_steps.is_empty() {
                    0.5
                } else {
                    reasoning_steps.iter().map(|s| s.confidence).sum::<f32>()
                        / reasoning_steps.len() as f32
                };

                // No truncation — preserve full LLM output for quality
                let summary_for_result = conclusion.clone();

                let decision_process = DecisionProcess {
                    situation_analysis,
                    data_collected,
                    reasoning_steps,
                    decisions,
                    conclusion,
                    confidence,
                    stop_reason: String::new(),
                };

                let success_rate = if actions_executed.is_empty() {
                    1.0
                } else {
                    let success_count =
                        actions_executed.iter().filter(|a| a.success).count() as f32;
                    success_count / actions_executed.len() as f32
                };

                let execution_result = StorageExecutionResult {
                    actions_executed,
                    report,
                    notifications_sent,
                    summary: summary_for_result,
                    success_rate,
                };

                Ok((decision_process, execution_result))
            }
        }
    }
}

// ========================================================================
// Tool-call text classification (malformed detection + orphan cleanup)
// ========================================================================

/// Result of inspecting LLM output text for tool-call XML fragments.
///
/// Small thinking-capable models (qwen3.6-35b-a3b MoE, etc.) sometimes emit
/// bare XML closing tags as content under complex multimodal + multi-round
/// context, with no corresponding opening tags and no native `tool_calls`
/// API field. This is model noise — must NOT be confused with a genuine
/// malformed tool-call attempt (which has both opening and closing tags).
pub(super) struct ToolCallTextClassification {
    /// True only when BOTH opening and closing tags are present — a real
    /// (but unparseable) tool-call attempt. Triggers the malformed-output
    /// error path so the user sees the failure surface.
    pub is_malformed: bool,
    /// True when closing tags appear WITHOUT opening tags — model noise.
    /// Caller should replace `final_text` with `cleaned_text`.
    pub has_orphan_closing_tags: bool,
    /// Text with orphan closing tags stripped (only meaningful when
    /// `has_orphan_closing_tags` is true; otherwise mirrors the input).
    pub cleaned_text: String,
}

/// Inspect LLM output text for tool-call XML fragments and decide whether
/// it's malformed (real attempt that failed to parse) or just orphan-tag
/// noise (closing tags only — common on small MoE thinking models under
/// complex multimodal context).
pub(super) fn classify_tool_call_text(text: &str) -> ToolCallTextClassification {
    let has_opening_tag =
        text.contains("<parameter") || text.contains("<function") || text.contains("<tool_call");
    let has_closing_tag = text.contains("</parameter>")
        || text.contains("</function>")
        || text.contains("</tool_call");

    let is_malformed = has_opening_tag && has_closing_tag;
    let has_orphan_closing_tags = has_closing_tag && !has_opening_tag;

    let cleaned_text = if has_orphan_closing_tags {
        text.replace("</parameter>", "")
            .replace("</function>", "")
            .replace("</tool_call>", "")
            .replace("</tool_call", "")
            .trim()
            .to_string()
    } else {
        text.to_string()
    };

    ToolCallTextClassification {
        is_malformed,
        has_orphan_closing_tags,
        cleaned_text,
    }
}

/// Heuristic: detect transient (retryable) failures from the error message.
///
/// These are failures where the agent itself is fine — the LLM provider had
/// a temporary hiccup (network, timeout, rate-limit). Keeping the agent in
/// Active status lets the scheduler retry on the next trigger.
///
/// Permanent failures (malformed output, auth/model errors, panics) return
/// false so the agent is demoted to Error and needs manual re-enable.
fn is_transient_failure(error: Option<&str>) -> bool {
    let msg = match error {
        Some(m) => m,
        None => return false, // unknown error → treat as permanent (safe)
    };
    let lower = msg.to_lowercase();
    // Network errors (reqwest::Error display)
    lower.contains("network")
        || lower.contains("error sending request")
        || lower.contains("dns")
        || lower.contains("connection reset")
        || lower.contains("connection refused")
        || lower.contains("broken pipe")
        // Timeouts (per-request and global 300s)
        || lower.contains("timed out")
        || lower.contains("timeout")
        // Rate limiting — match the actual strings our backends produce
        || lower.contains("rate limit")
        || lower.contains("too many requests")
    // Note: "429" alone is intentionally NOT matched — it's too generic
    // and could appear in device IDs / ports. Backends emit "Rate limited"
    // or "too many requests" as the human-readable message instead.
}

#[cfg(all(test, feature = "test-utils"))]
mod behavior_tests;

#[cfg(test)]
mod tests {
    use super::{classify_tool_call_text, is_transient_failure};

    #[test]
    fn transient_network_error_is_detected() {
        assert!(is_transient_failure(Some("LLM error: Network error: error sending request for url (https://dashscope.aliyuncs.com/...)")));
    }

    #[test]
    fn transient_timeout_is_detected() {
        assert!(is_transient_failure(Some("Execution timed out after 300s")));
        assert!(is_transient_failure(Some("LLM error: request timeout")));
    }

    #[test]
    fn transient_rate_limit_is_detected() {
        assert!(is_transient_failure(Some("LLM error: Rate limited by API")));
        assert!(is_transient_failure(Some("429 Too Many Requests")));
    }

    #[test]
    fn transient_connection_reset_is_detected() {
        assert!(is_transient_failure(Some("connection reset by peer")));
        assert!(is_transient_failure(Some("connection refused")));
    }

    #[test]
    fn permanent_malformed_output_is_not_transient() {
        assert!(!is_transient_failure(Some(
            "LLM error: LLM tool-calling produced malformed output"
        )));
    }

    #[test]
    fn permanent_panic_is_not_transient() {
        assert!(!is_transient_failure(Some(
            "Execution panic: index out of bounds"
        )));
    }

    #[test]
    fn number_429_alone_does_not_trigger_false_positive() {
        // "429" should NOT match by itself — it could be a device id / port
        assert!(!is_transient_failure(Some("Device 429 not found")));
        assert!(!is_transient_failure(Some("port 4429 unavailable")));
    }

    #[test]
    fn none_error_is_not_transient() {
        assert!(!is_transient_failure(None));
    }

    #[test]
    fn normal_text_is_not_malformed() {
        let c = classify_tool_call_text("Looks normal — no anomalies detected.");
        assert!(!c.is_malformed);
        assert!(!c.has_orphan_closing_tags);
        assert_eq!(c.cleaned_text, "Looks normal — no anomalies detected.");
    }

    #[test]
    fn empty_text_is_not_malformed() {
        let c = classify_tool_call_text("");
        assert!(!c.is_malformed);
        assert!(!c.has_orphan_closing_tags);
    }

    #[test]
    fn real_malformed_attempt_with_open_and_close_tags_detected() {
        // Both opening and closing tags = a real but unparseable tool-call
        // attempt. Must surface as malformed so user sees the failure.
        let text = r#"<tool_call>{"name":"foo"}</tool_call>"#;
        let c = classify_tool_call_text(text);
        assert!(c.is_malformed);
        assert!(!c.has_orphan_closing_tags); // not orphan — has opening
    }

    #[test]
    fn orphan_closing_tags_stripped_as_model_noise() {
        // Reproduces qwen3.6-35b-a3b production failure on prod-01 (agent
        // e64fb295 Garbage Monitoring, event:values.image trigger):
        // model emits bare `</parameter></function></tool_call>` as content,
        // no opening tags, no native tool_calls API field. Must be treated
        // as noise and stripped, NOT surfaced as malformed-output error.
        let text = "\n</parameter>\n</function>\n</tool_call>";
        let c = classify_tool_call_text(text);
        assert!(!c.is_malformed, "orphan tags must not trigger malformed");
        assert!(
            c.has_orphan_closing_tags,
            "orphan closing tags must be detected"
        );
        assert_eq!(c.cleaned_text, "");
    }

    #[test]
    fn orphan_closing_tags_with_leading_text_preserves_text() {
        let text = "Anomaly detected.</parameter></function>";
        let c = classify_tool_call_text(text);
        assert!(!c.is_malformed);
        assert!(c.has_orphan_closing_tags);
        assert_eq!(c.cleaned_text, "Anomaly detected.");
    }

    #[test]
    fn opening_tag_only_not_malformed() {
        // Half-formed attempt without closing — not detected as malformed
        // (parser may still extract a tool call from incomplete input).
        let text = "I will call <tool_call>";
        let c = classify_tool_call_text(text);
        assert!(!c.is_malformed);
        assert!(!c.has_orphan_closing_tags);
    }
}
