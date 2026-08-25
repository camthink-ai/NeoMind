// ========== AI Agent Types ==========
// Must match backend AgentDto and related types (crates/api/src/handlers/agents.rs)

/**
 * AI Agent status enumeration
 */
export type AgentStatus = 'Active' | 'Paused' | 'Error' | 'Executing'

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
  created_at: string
  last_execution_at: string | null
  execution_count: number
  success_count: number
  error_count: number
  avg_duration_ms: number
  llm_backend_id?: string
  // Advanced configuration (returned by list API)
  max_chain_depth?: number
  priority?: number
  context_window_size?: number
  execution_mode?: 'focused' | 'free'
}

/**
 * AI Agent detail - matches backend AgentDetailDto
 */
export interface AiAgentDetail extends AiAgent {
  user_prompt: string
  parsed_intent?: ParsedIntent
  memory?: AgentMemory
  resources: AgentResource[]
  schedule: AgentSchedule
  stats: AgentStats
  updated_at: string
  error_message?: string
  user_messages: UserMessage[]
  context_window_size: number
  // Tool chaining configuration
  max_chain_depth?: number
  priority?: number
  /** Execution mode: "focused" for bound-resource agents, "free" for unrestricted tool-calling */
  execution_mode?: 'focused' | 'free'
  /** Custom system prompt override */
  system_prompt?: string
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
export interface AgentExecution {
  id: string
  agent_id: string
  timestamp: string
  trigger_type: string
  status: ExecutionStatus
  duration_ms: number
  error?: string
}

/**
 * Execution status for agent executions
 */
export type ExecutionStatus = 'Running' | 'Completed' | 'Failed' | 'Cancelled'

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
  /** Execution mode: "focused" for bound-resource agents, "free" for unrestricted tool-calling */
  execution_mode?: 'focused' | 'free'
  /** Custom system prompt override */
  system_prompt?: string
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
 * Request to update an agent - all fields optional, matches backend UpdateAgentRequest
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
  /** Execution mode: "focused" for bound-resource agents, "free" for unrestricted tool-calling */
  execution_mode?: 'focused' | 'free'
  /** Custom system prompt override */
  system_prompt?: string
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
 * Agent execution detail with full decision process
 * Extends AgentExecution with decision_process and result
 */
export interface AgentExecutionDetail extends AgentExecution {
  decision_process?: DecisionProcess
  result?: ExecutionResult
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
  confidence: number
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
  confidence: number
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
