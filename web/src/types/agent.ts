// ========== AI Agent Types ==========
// Must match backend AgentDto and related types (crates/api/src/handlers/agents.rs)

/**
 * AI Agent status enumeration
 */
export type AgentStatus = 'Active' | 'Paused' | 'Error' | 'Executing' | 'Stopped' | 'Completed'

/**
 * Schedule type for agent execution
 */
export type AgentScheduleType = 'interval' | 'cron' | 'event' | 'manual'

/**
 * Resource type for agent resources (lowercase to match backend)
 */
export type AgentResourceType = 'device' | 'metric' | 'command' | 'extension_tool' | 'extension_metric' | 'data_stream'

/**
 * AI Agent list item - matches backend AgentDto
 */
export interface AiAgent {
  id: string
  name: string
  status: AgentStatus
  description?: string
  user_prompt?: string
  created_at: string | number
  last_execution_at: string | number | null
  execution_count: number
  success_count: number
  error_count: number
  avg_duration_ms?: number
  llm_backend_id?: string
  config_version?: number
  preset?: AgentPresetId
  trigger?: TriggerSpec
  tool_policy?: ToolPolicy
  // Advanced configuration (returned by list API)
  max_chain_depth?: number
  priority?: number
  context_window_size?: number
  /** Contract field names — card role badge + latest-output keys */
  output_fields?: string[]
  /** Latest published ai:{id}:{field} values (list endpoint, best-effort) */
  latest_output?: Record<string, unknown>
  /**
   * The same readings with what it takes to check them: when each was
   * published, the model's own confidence, and the run that produced it.
   * Absent pieces are omitted rather than nulled.
   */
  latest_output_state?: Record<string, OutputFieldState>
  /** Last error message (Error status) */
  error?: string
  execution_mode?: AgentExecutionMode
  /** Memory axis — absent means the mode-derived default */
  memory_mode?: AgentMemoryMode
  notify?: NotifyPolicy | AgentNotify
  /** Tool gating for this agent (None = default: tools enabled, all allowed). */
  tool_config?: AgentToolConfig
}

/**
 * AI Agent detail - matches backend GET /api/agents/:id (AgentSpec + runtime status/stats/effective_plan)
 */
export interface AiAgentDetail extends AiAgent {
  user_prompt: string
  updated_at: string | number
  /** New AgentSpec contract fields */
  trigger?: TriggerSpec
  pacing?: PacingPolicy
  input?: InputContract
  tool_policy?: ToolPolicy
  output?: OutputContract
  notify?: NotifyPolicy | AgentNotify
  controlled_actions?: ControlledAction[]
  effective_plan?: EffectivePlan
  config_version?: number
  preset?: AgentPresetId
  /** Runtime statistics (AgentRuntimeStats from agent_runtime) */
  stats?: Partial<AgentRuntimeStats> & Partial<AgentStats>
  error_message?: string
  /** Optional/legacy fields */
  parsed_intent?: ParsedIntent
  memory?: AgentMemory
  resources?: AgentResource[]
  schedule?: AgentSchedule
  user_messages?: UserMessage[]
  context_window_size?: number
  max_chain_depth?: number
  priority?: number
  execution_mode?: AgentExecutionMode
  system_prompt?: string
  output_schema?: OperatorField[]
  operator_config?: OperatorConfig
  memory_mode?: AgentMemoryMode
  tool_config?: AgentToolConfig
}

/**
 * Agent schedule configuration
 */
export interface AgentSchedule {
  schedule_type: 'interval' | 'cron' | 'event' | 'manual'
  interval_seconds?: number
  cron_expression?: string
  timezone?: string
  event_filter?: string
}

/**
 * Parsed intent from natural language prompt
 */
export interface ParsedIntent {
  intent_type: string
  target_metrics: string[]
  conditions: string[]
  actions: string[]
  confidence: number
}

/**
 * Agent memory with persistent state
 */
/**
 * Agent memory: execution journal + knowledge file index
 */
export interface AgentMemory {
  journal: ExecutionJournal
  knowledge_files: KnowledgeFileRef[]
  updated_at: string
}

/**
 * Execution journal — recent execution records
 */
export interface ExecutionJournal {
  records: JournalExecutionRecord[]
  max_records: number
}

/**
 * A single execution record in the agent's journal
 */
export interface JournalExecutionRecord {
  timestamp: string
  execution_id: string
  outcome: string
  action_taken: string
  success: boolean
}

/**
 * Reference to a knowledge file created by the agent
 */
export interface KnowledgeFileRef {
  name: string
  description: string
  content?: string
  created_at: string
  updated_at: string
}

/**
 * Agent execution statistics
 */
export interface AgentStats {
  total_executions: number
  successful_executions: number
  failed_executions: number
  avg_duration_ms: number
}

/**
 * Available resources for an Agent
 * This represents all devices, metrics, and commands that the AI can use
 */
export interface AgentAvailableResources {
  /**
   * All devices in the system
   */
  devices: Array<{
    id: string
    name: string
    type: string
    online: boolean
    capabilities: string[]
  }>

  /**
   * All available metrics grouped by device
   */
  metrics: Array<{
    device_id: string
    device_name: string
    metrics: Array<{
      name: string
      display_name: string
      unit?: string
      data_type: string
    }>
  }>

  /**
   * All available commands grouped by device
   */
  commands: Array<{
    device_id: string
    device_name: string
    commands: Array<{
      name: string
      display_name: string
      parameters?: Record<string, unknown>
    }>
  }>

  /**
   * Summary counts
   */
  summary: {
    total_devices: number
    online_devices: number
    total_metrics: number
    total_commands: number
  }
}

/**
 * Agent execution record - matches backend AgentExecutionDto
 */
/** One published output field, as the detail view shows it. */
export interface OutputFieldState {
  value: unknown
  /** Unix seconds. */
  at: number
  confidence?: number
  /** The run that produced it — the way back to its evidence. */
  execution_id?: string
}

/** What the run was given — mirrors backend `ExecutionInputSummaryDto`. */
export interface ExecutionInputSummary {
  metrics_collected: number
  image_extracted?: boolean
  image_source_field?: string
  /** `inline_multimodal` | `vision_tool` | `none` */
  vision_path?: string
  /** `event_image_unreadable` | `report_image_unreadable` | `no_usable_event_data` */
  skip_reason?: string
}

export interface AgentExecution {
  id: string
  agent_id: string
  timestamp: string
  trigger_type: string
  status: ExecutionStatus
  duration_ms: number
  /** List DTO conclusion (V2 nests here; detail may also expose decision_process). */
  conclusion?: string
  error?: string
  input_summary?: ExecutionInputSummary
}

/**
 * Execution status for agent executions
 */
export type ExecutionStatus = 'Running' | 'Completed' | 'Failed' | 'Partial' | 'Cancelled'

/**
 * User message sent to an agent between executions
 */
export interface UserMessage {
  id: string
  timestamp: number
  content: string
  message_type?: string
}

/**
 * Agent resource for devices, metrics, and commands
 */
export interface AgentResource {
  resource_type: AgentResourceType
  resource_id: string
  name: string
  config: Record<string, unknown>
}

/**
 * Request to create a new AI Agent - matches backend CreateAgentRequest
 */
/** Structured (L0) field type: number | text | boolean | enum(values) */
export type OperatorFieldType =
  | { type: 'number' }
  | { type: 'text' }
  | { type: 'boolean' }
  | { type: 'enum'; values: string[] }

/** One output field of a structured agent — published as ai:{agentId}:{name} */
export interface OperatorField {
  name: string
  field_type: OperatorFieldType
  unit?: string
  description?: string
}

/** When an agent notifies (mirrors the server's `NotifyOn`). */
export type AgentNotifyOn = 'failure' | 'always' | 'judgment'

/** Explicit notification routing (mirrors the server's `AgentNotify`). */
export interface AgentNotify {
  channels: string[]
  on: AgentNotifyOn
}

/**
 * How much history an agent carries into each run (mirrors the server's
 * `MemoryMode`). `tool` = judged fresh each time; `assistant` = carries the
 * recent-execution narrative. Omitted = derived from the execution mode.
 */
export type AgentMemoryMode = 'tool' | 'assistant'

/** Structured (L0) runtime tuning */
export interface OperatorConfig {
  debounce_secs?: number
  max_calls_per_day?: number
  timeout_secs?: number
  consecutive_failure_threshold?: number
  /** Event-trigger cooldown in seconds (undefined = platform default 60; 0 = on-demand) */
  event_cooldown_secs?: number
}

export type AgentExecutionMode = 'focused' | 'free' | 'structured'

/** Dry-run (试跑) result — one inference, nothing published */
export interface DryRunResult {
  agent_id: string
  context: string
  data_sources: number
  fields: Record<string, unknown>
  raw_text: string
  attempts: number
}

/** Save-before dry-run request — transient agent, nothing persisted */
export interface TestPreviewRequest {
  user_prompt: string
  /** Preview mode; omitted = structured (the original dry-run). */
  execution_mode?: AgentExecutionMode
  resources?: ResourceRequest[]
  output_schema?: OperatorField[]
  operator_config?: OperatorConfig
  memory_mode?: AgentMemoryMode
  notify?: AgentNotify
  llm_backend_id?: string
  /** Preview fidelity: run the preview with the same tool gating the saved agent will have. */
  tool_config?: AgentToolConfig
}

/**
 * @deprecated Legacy create body. `agentsApi.createAgent` compiles this into
 * an AgentSpecDraft and POSTs `/agents/v2`. Prefer `AgentSpecDraft` + createAgentV2.
 */
export interface CreateAgentRequest {
  name: string
  description?: string
  user_prompt: string
  device_ids?: string[]
  metrics?: MetricSelectionRequest[]
  commands?: CommandSelectionRequest[]
  resources?: ResourceRequest[]
  schedule: AgentScheduleRequest
  llm_backend_id?: string
  /** Enable tool chaining (default: false) */
  /** Maximum chain depth (default: 3) */
  max_chain_depth?: number
  /** Agent priority 0-255 (default: 128) */
  priority?: number
  /** Context window size (default: 10) */
  context_window_size?: number
  /** Execution mode: "focused" for bound-resource agents, "free" for unrestricted tool-calling, "structured" for L0 extraction */
  execution_mode?: AgentExecutionMode
  /** Custom system prompt override */
  system_prompt?: string
  /** Structured (L0) output contract */
  output_schema?: OperatorField[]
  /** Structured (L0) runtime tuning */
  operator_config?: OperatorConfig
  memory_mode?: AgentMemoryMode
  notify?: AgentNotify
  /** Tool gating: enabled=false → text-only agent; allowed_tools empty → all. */
  tool_config?: AgentToolConfig
}

/**
 * Unified resource request format (supports devices and extensions)
 */
export interface ResourceRequest {
  resource_id: string
  resource_type: 'device' | 'metric' | 'command' | 'extension_tool' | 'extension_metric' | 'data_stream'
  name: string
  config?: Record<string, unknown>
}

/**
 * Metric selection in create request
 */
export interface MetricSelectionRequest {
  device_id: string
  metric_name: string
  display_name: string
  /** Data collection configuration for this metric */
  config?: {
    data_collection?: {
      time_range_minutes?: number
      include_history?: boolean
      max_points?: number
      include_trend?: boolean
      include_baseline?: boolean
    }
  }
}

/**
 * Command selection in create request
 */
export interface CommandSelectionRequest {
  device_id: string
  command_name: string
  display_name: string
  parameters: Record<string, unknown>
}

/**
 * Agent schedule in create request
 */
export interface AgentScheduleRequest {
  schedule_type: AgentScheduleType
  interval_seconds?: number
  cron_expression?: string
  event_filter?: string
  timezone?: string
}

/**
 * @deprecated Legacy update body. `agentsApi.updateAgent` compiles this into
 * an AgentSpecPatch and PUTs `/agents/v2/:id`. Prefer `AgentSpecPatch` + updateAgentV2.
 */
export interface UpdateAgentRequest {
  name?: string
  description?: string
  user_prompt?: string
  status?: string
  llm_backend_id?: string
  /** Schedule configuration */
  schedule?: AgentScheduleRequest
  /** New resource format */
  resources?: ResourceRequest[]
  /** Legacy device IDs (for backward compatibility) */
  device_ids?: string[]
  /** Legacy metric selections */
  metrics?: MetricSelectionRequest[]
  /** Legacy command selections */
  commands?: CommandSelectionRequest[]
  /** Enable tool chaining (default: false) */
  /** Maximum chain depth (default: 3) */
  max_chain_depth?: number
  /** Agent priority 1-10 (default: 5) */
  priority?: number
  /** Context window size in tokens (default: 8192) */
  context_window_size?: number
  /** Execution mode: "focused" for bound-resource agents, "free" for unrestricted tool-calling, "structured" for L0 extraction */
  execution_mode?: AgentExecutionMode
  /** Custom system prompt override */
  system_prompt?: string
  /** Structured (L0) output contract */
  output_schema?: OperatorField[]
  /** Structured (L0) runtime tuning */
  operator_config?: OperatorConfig
  memory_mode?: AgentMemoryMode
  notify?: AgentNotify
  /** Tool gating: enabled=false → text-only agent; allowed_tools empty → all. */
  tool_config?: AgentToolConfig
}

/**
 * Request to execute an agent
 */
export interface ExecuteAgentRequest {
  trigger_type?: string
  event_data?: Record<string, unknown>
  /** Text input from the caller */
  input?: string
  /** Structured data from the caller (e.g., images) */
  data?: Record<string, unknown>
}

/**
 * Agent list response
 */
export interface AgentListResponse {
  agents: AiAgent[]
  count: number
}

/**
 * Agent executions response
 */
export interface AgentExecutionsResponse {
  agent_id: string
  executions: AgentExecution[]
  count: number
}

/**
 * Agent runtime statistics — mirrors backend `AgentRuntimeStats`.
 */
export interface AgentRuntimeStats {
  total_runs: number
  successful_runs: number
  failed_runs: number
  partial_runs?: number
  skipped_daily_limit?: number
  skipped_permit_timeout?: number
  consecutive_failures?: number
  runs_today?: number
  quota_day?: string | null
  last_execution_at?: number | null
  last_execution_id?: string | null
  avg_duration_ms?: number
  last_duration_ms?: number | null
  error_message?: string | null
}

/**
 * Delivery receipt state — mirrors backend `DeliveryState`.
 */
export type DeliveryState =
  | { state: 'intent_pending' }
  | { state: 'rejected'; reason: string }
  | { state: 'accepted' }
  | { state: 'confirmed' }
  | { state: 'outcome_unknown'; reason: string }

/**
 * Side-effect delivery receipt — mirrors backend `ActionDeliveryRecord`.
 */
export interface ActionDeliveryRecord {
  execution_id: string
  destination: string
  action_fingerprint: string
  depends_on?: string[]
  idempotent: boolean
  state: DeliveryState
}

/**
 * Response from GET /api/agents/:id/explain-last.
 */
export interface ExplainLastResponse {
  agent_id: string
  explanation: string
}

/**
 * Agent execution detail with full decision process
 * Extends AgentExecution with decision_process, result, structured_result, deliveries, alert_verdict
 */
export interface AgentExecutionDetail extends AgentExecution {
  decision_process?: DecisionProcess
  result?: ExecutionResult
  /** Top-level brief text / conclusion returned by GET /api/agents/:id/executions/:exec_id. */
  brief_text?: string
  /** Structured JSON fields extracted under OutputContract. */
  structured_result?: Record<string, unknown> | null
  /** Alert verdict produced by the run (when applicable). */
  alert_verdict?: string | null
  /** Side-effect / notification delivery receipts recorded for this run. */
  deliveries?: ActionDeliveryRecord[]
  /** AgentSpec config_version active when this execution ran. */
  config_version?: number
  /** Whether this run was a dry-run trial. */
  dry_run?: boolean
  /** True when agent config changed while this run was in flight. */
  stale_config?: boolean | null
}

/**
 * AI decision process with full reasoning trace
 */
export interface DecisionProcess {
  situation_analysis: string
  data_collected: DataCollected[]
  reasoning_steps: ReasoningStep[]
  decisions: Decision[]
  conclusion: string
  /**
   * The model's own confidence, 0–1. Absent when it reported none — the
   * backend omits the key rather than sending `null`, because these consumers
   * guard on `undefined` and `null * 100` would render as "0%".
   */
  confidence?: number
  /** Why the run stopped; omitted when empty. */
  stop_reason?: string
  /** Summed LLM tokens when reported; omitted when 0/unreported. */
  tokens_used?: number
}

/**
 * Data collected for decision making
 */
export interface DataCollected {
  source: string
  data_type: string
  values: Record<string, unknown> | unknown[]
  timestamp: number
}

/**
 * A single reasoning step in the decision process
 */
export interface ReasoningStep {
  step_number: number
  description: string
  step_type: string
  input?: string
  output: string
  confidence?: number
}

/**
 * A decision made during execution
 */
export interface Decision {
  decision_type: string
  description: string
  action: string
  rationale: string
  expected_outcome: string
}

/**
 * Execution result with actions taken
 */
export interface ExecutionResult {
  actions_executed: ActionExecuted[]
  report?: string
  notifications_sent: NotificationSent[]
  summary: string
  success_rate: number
}

/**
 * An action that was executed
 */
export interface ActionExecuted {
  action_type: string
  description: string
  target: string
  success: boolean
  /** Parameters used for the action (e.g., command payload) */
  parameters?: Record<string, unknown>
  /** Result message or error details */
  result?: string
}

/**
 * A generated report
 */
export interface GeneratedReport {
  report_type: string
  content: string
  data_summary: DataSummary[]
  generated_at: number
}

/**
 * Summary of data included in report
 */
export interface DataSummary {
  source: string
  metric: string
  count: number
  statistics: Record<string, unknown>
}

/**
 * A notification that was sent
 */
export interface NotificationSent {
  channel: string
  recipient: string
  message: string
  sent_at: number
  success: boolean
}

/**
 * Read-only catalog item describing a tool in the server's ToolRegistry.
 * Returned by GET /api/agents/tools.
 */
/**
 * Per-agent tool gating. `enabled: false` → the agent gets NO tools
 * (text-only responses); `allowed_tools` empty → all available tools.
 */
export interface AgentToolConfig {
  enabled: boolean
  allowed_tools: string[]
}

export interface AgentToolCatalogItem {
  name: string
  description: string
  /** Origin of the tool: built-in | extension */
  source: string
  namespace?: string | null
  category: string
  /** JSON Schema describing the tool's input parameters */
  parameters: Record<string, unknown>
  deprecated?: boolean
  version?: string
  /** True when this tool is hidden from the LLM (disabled via Extensions page). */
  disabled?: boolean
}

// =============================================================================
// Agent contract v2 (clean-slate) — mirrors crates/neomind-ai-agent/src/model
// Temporary until M3 rewrites the editor against these types directly.
// =============================================================================

/** Official task-family preset id. */
export type AgentPresetId = 'daily_brief' | 'partial_meter' | 'event_vision' | 'custom'

export type InputModality =
  | 'metric'
  | 'time_series'
  | 'structured_json'
  | 'text'
  | 'image'
  | 'caller_input'

export type InputSourceRef =
  | { kind: 'device_metric'; device_id: string; field: string }
  | { kind: 'whole_device'; device_id: string }
  | { kind: 'extension_instance'; id: string }
  | { kind: 'upstream_ai'; agent_id: string; field: string }
  | { kind: 'caller_payload' }

export type FreshnessPolicy =
  | { kind: 'same_report' }
  | { kind: 'latest_within'; max_age_secs: number }
  | { kind: 'lookback_window'; lookback_secs: number; max_points: number }
  | { kind: 'any' }

export type MissingPolicy = 'undetermined' | 'report_stale' | 'skip' | 'ignore'

export interface InputRequirement {
  key: string
  source: InputSourceRef
  operation?: string
  params?: Record<string, unknown>
  auth_scope?: string
  timeout_secs: number
  result_type: InputModality
  freshness: FreshnessPolicy
  on_missing: MissingPolicy
}

export interface InputContract {
  requirements: InputRequirement[]
}

/** Externally-tagged ToolPolicy (unit variants are bare strings). */
export type ToolPolicy =
  | 'disabled'
  | 'unrestricted'
  | { scoped: { allowed_tools: string[]; allowed_commands?: string[] } }

export interface OutputFieldV2 {
  name: string
  depends_on?: string[]
  description?: string
}

export interface OutputContract {
  fields: OutputFieldV2[]
  publish?: boolean
}

/** Three-way notify policy (complete / alert / failure). */
export interface NotifyPolicy {
  channels: string[]
  on_complete?: boolean
  on_alert?: boolean
  alert_depends_on?: string[]
  on_failure?: boolean
  pending_confirmation?: boolean
}

export type TriggerSpec =
  | { kind: 'cron'; expr: string; timezone: string }
  | { kind: 'interval'; every_secs: number }
  | { kind: 'event'; filter?: unknown }
  | { kind: 'manual' }

export interface PacingPolicy {
  burst_merge_secs?: number
  min_interval_secs?: number
  max_runs_per_day?: number
  failure_threshold?: number
}

export interface ControlledAction {
  name: string
  destination: string
  depends_on?: string[]
  idempotent?: boolean
  params?: Record<string, unknown>
}

/** Create/update request DTO (no id / config_version). */
export interface AgentSpecDraft {
  name: string
  description?: string
  user_prompt: string
  llm_backend_id?: string
  preset?: AgentPresetId
  trigger: TriggerSpec
  pacing?: PacingPolicy
  input: InputContract
  tool_policy: ToolPolicy
  output?: OutputContract
  notify?: NotifyPolicy
  controlled_actions?: ControlledAction[]
}

/** Frozen static configuration for a managed agent. */
export interface AgentSpec extends AgentSpecDraft {
  id: string
  config_version: number
  created_at: number
  updated_at: number
}

/** Partial update: omitted fields keep previous values. */
export interface AgentSpecPatch {
  name?: string
  description?: string | null
  user_prompt?: string
  llm_backend_id?: string | null
  preset?: AgentPresetId | null
  trigger?: TriggerSpec
  pacing?: PacingPolicy
  input?: InputContract
  tool_policy?: ToolPolicy
  output?: OutputContract
  notify?: NotifyPolicy
  controlled_actions?: ControlledAction[]
}

export interface PlanRead {
  key: string
  source_id: string
  operation?: string | null
  auth_scope?: string | null
  capability_class: string
}

export interface ExternalReadCounts {
  weather: number
  tariff: number
  other: number
}

/** Static EffectivePlan — zero external reads, zero side effects. */
export interface EffectivePlan {
  agent_id: string
  config_version: number
  trigger_summary: string
  timezone?: string | null
  authorized_reads: PlanRead[]
  tool_policy: ToolPolicy
  notify: NotifyPolicy
  controlled_actions: ControlledAction[]
  warnings?: string[]
  external_read_counts: ExternalReadCounts
}

export type EvidenceQuality =
  | 'fresh'
  | 'stale'
  | 'missing'
  | { error: { reason: string } }

export type EvidenceDetail =
  | { kind: 'vision'; image_source_field: string; vision_path: string }
  | { kind: 'time_series'; points: number; value?: unknown }
  | { kind: 'structured'; value: unknown }
  | { kind: 'text'; text: string }

export interface EvidenceItem {
  key: string
  source_id: string
  result_type: InputModality
  observed_at?: number | null
  valid_window?: [number, number] | null
  fetched_at: number
  quality: EvidenceQuality
  binding_proof: string
  detail?: EvidenceDetail | null
}

export interface EvidenceReport {
  items: EvidenceItem[]
  missing_keys?: string[]
  skip_reason?: string | null
}

/**
 * Opaque v2 fields the legacy editor cannot fully express. Stashed by
 * `v2SpecToFormState` and restored by `formStateToV2Draft` so a name-only
 * edit does not wipe inputs / notify / tool_policy / timezone.
 */
export interface AgentSpecV2Carry {
  preset?: AgentPresetId
  trigger: TriggerSpec
  pacing?: PacingPolicy
  input: InputContract
  tool_policy: ToolPolicy
  output?: OutputContract
  notify?: NotifyPolicy
  controlled_actions?: ControlledAction[]
}

/**
 * Legacy-editor form shape used by the M2 temporary adapter. Compatible with
 * CreateAgentRequest fields the old editor already mutates, plus `v2Carry`.
 */
export interface AgentEditorFormState {
  name: string
  description?: string
  user_prompt: string
  llm_backend_id?: string
  schedule: AgentScheduleRequest
  resources?: ResourceRequest[]
  device_ids?: string[]
  metrics?: MetricSelectionRequest[]
  commands?: CommandSelectionRequest[]
  execution_mode?: AgentExecutionMode
  system_prompt?: string
  output_schema?: OperatorField[]
  operator_config?: OperatorConfig
  memory_mode?: AgentMemoryMode
  /** Legacy notify (`on: failure|always|judgment`). */
  notify?: AgentNotify
  tool_config?: AgentToolConfig
  max_chain_depth?: number
  priority?: number
  context_window_size?: number
  /** Round-trip bag for v2-only / lossy fields. */
  v2Carry?: AgentSpecV2Carry
}
