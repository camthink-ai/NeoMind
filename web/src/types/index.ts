// ========== User Authentication Types ==========

export type UserRole = 'admin' | 'user' | 'viewer'

export interface UserInfo {
  id: string
  username: string
  role: UserRole
  created_at: number
}

export interface LoginRequest {
  username: string
  password: string
}

export interface LoginResponse {
  token: string
  user: UserInfo
}

export interface RegisterRequest {
  username: string
  password: string
  role?: UserRole
}

export interface ChangePasswordRequest {
  old_password: string
  new_password: string
}

// API Types
export interface Device {
  id: string
  device_id: string  // Same as id, included for backend compatibility
  name?: string
  device_type: string
  status: string
  last_seen: string
  current_values?: Record<string, unknown>
  // Associated plugin information
  plugin_id?: string
  plugin_name?: string
  adapter_type?: 'mqtt' | 'modbus' | 'hass' | 'http' | 'custom'
  // Device config (contains HASS state topic mappings for HASS devices)
  config?: Record<string, string>
  // Metric and command counts
  metric_count?: number
  command_count?: number
}

export interface DeviceType {
  device_type: string
  name: string
  description: string
  categories: string[]
  metric_count: number
  command_count: number
  uplink?: {
    metrics: MetricDefinition[]
  }
  downlink?: {
    commands: CommandDefinition[]
  }
}

export interface MetricDefinition {
  name: string
  display_name: string
  topic: string
  value_template?: string
  data_type: string
  unit?: string
  min?: number
  max?: number
  required?: boolean
}

export interface CommandDefinition {
  name: string
  display_name?: string
  topic: string
  payload_template: string
  parameters?: ParameterDefinition[]
  response_topic?: string
  timeout_ms?: number
}

export interface ParameterDefinition {
  name: string
  display_name?: string
  data_type: string
  default_value?: MetricValue
  min?: number
  max?: number
  unit?: string
  allowed_values?: MetricValue[]
}

export type MetricValue =
  | { Integer: number }
  | { Float: number }
  | { String: string }
  | { Boolean: boolean }
  | { Null: null }

// Alert type - must match backend AlertDto (crates/api/src/handlers/alerts.rs)
export interface Alert {
  id: string
  title: string
  message: string
  severity: 'info' | 'warning' | 'critical' | 'emergency' | string
  status: 'active' | 'acknowledged' | 'resolved' | string
  acknowledged: boolean
  // Backend sends 'timestamp' (ISO 8601 string)
  // Frontend may also use 'created_at' (number timestamp)
  timestamp?: string
  created_at?: number | string
  source?: string
}

// Chat Types
export interface Message {
  id: string
  role: 'user' | 'assistant' | 'system'
  content: string
  timestamp: number
  thinking?: string
  tool_calls?: ToolCall[]
}

export interface ToolCall {
  id: string
  name: string
  arguments: Record<string, unknown>
  result?: unknown
}

// Session Types
export interface ChatSession {
  sessionId: string
  id: string
  createdAt: number
  updatedAt?: number
  messageCount?: number
  // User-defined title for the session
  title?: string | null
  // Preview text from last message
  preview?: string
}

export interface SessionListResponse {
  sessions: ChatSession[]
  meta: {
    page: number
    pageSize: number
    totalCount: number
    totalPages: number
  }
}

export interface SessionHistoryResponse {
  messages: Message[]
  count: number
}

// Server WebSocket message types (matching backend)
//
// NOTE: These types must match the AgentEvent serialization in crates/agent/src/agent/types.rs
// and the WebSocket message format in crates/api/src/handlers/sessions.rs
export type ServerMessage =
  | { type: 'system'; content?: string; sessionId?: string }
  | { type: 'session_created'; sessionId: string }
  | { type: 'session_switched'; sessionId: string }
  // Thinking content - AI reasoning process
  | { type: 'Thinking'; content: string; sessionId: string }
  // Actual response content
  | { type: 'Content'; content: string; sessionId: string }
  // Tool call starting
  | { type: 'ToolCallStart'; tool: string; arguments: Record<string, unknown>; sessionId: string }
  // Tool call completed - result is a string (JSON or plain text)
  | { type: 'ToolCallEnd'; tool: string; result: string; sessionId: string; success?: boolean }
  // Error occurred - sessionId is always included when sent from backend
  | { type: 'Error'; message: string; sessionId: string }
  // Stream ended
  | { type: 'end'; sessionId: string }
  // Non-streaming response (fallback)
  | { type: 'response'; content: string; sessionId: string; toolsUsed?: string[]; processingTimeMs?: number }
  // Device status update
  | { type: 'device_update'; updateType: string; deviceId: string; status?: string; lastSeen?: number }

// Client WebSocket message types
export interface ClientChatMessage {
  message: string
  sessionId?: string
}

// Settings Types
export interface LlmSettings {
  backend: string
  endpoint?: string
  api_key?: string
  model: string
}

export interface MqttSettings {
  listen: string
  port: number
  discovery_prefix: string
  auto_discovery: boolean
}

export interface ExternalBroker {
  id: string
  name: string
  broker: string
  port: number
  tls: boolean
  username?: string
  password?: string
  ca_cert?: string
  client_cert?: string
  client_key?: string
  enabled: boolean
  connected?: boolean
  last_error?: string
  updated_at: number
}

// Data Source Types
export interface MqttStatus {
  connected: boolean
  listen_address: string
  subscriptions_count: number
  devices_count: number
  clients_count: number
  server_ip: string
  listen_port: number
  external_brokers?: ExternalBrokerConnection[]
}

export interface ExternalBrokerConnection {
  id: string
  name: string
  broker: string
  port: number
  tls: boolean
  connected: boolean
  enabled: boolean
  last_error?: string
}

export interface ApiResponse<T> {
  success?: boolean
  data?: T
  error?: string
  message?: string
}

// Request Types
export interface AddDeviceRequest {
  device_type: string
  device_id?: string
  name?: string
}

export interface SendCommandRequest {
  params: Record<string, unknown>
}

// Discovery Types
export interface DiscoveredDevice {
  id: string
  device_type: string | null
  host: string
  port: number
  confidence: number
  info: Record<string, string>
}

// HASS Discovery Types
export interface HassDiscoveryStatus {
  hass_discovery: {
    enabled: boolean
    subscription_topic: string
    description: string
  }
  supported_components: Array<{
    component: string
    device_type: string | null
  }>
  component_count: number
}

export interface HassDiscoveryRequest {
  broker?: string
  port?: number
  components?: string[]
  auto_register?: boolean
}

export interface HassDiscoveryResponse {
  message: string
  subscription_topic: string
  components: string[]
  auto_register: boolean
  instructions: {
    subscribe: string
    wait_for_messages: string
    auto_register: string
  }
}

// HASS entity information (part of an aggregated device)
export interface HassEntity {
  entity_id: string
  name?: string
  component: string
  metric_count: number
  command_count: number
}

// Aggregated HASS device (physical device with multiple entities)
export interface HassDiscoveredDevice {
  device_id: string
  name?: string
  description: string
  entity_count: number
  total_metrics: number
  total_commands: number
  entities: HassEntity[]
  already_registered: boolean
  device_info: Record<string, string>
}

export interface HassProcessRequest {
  topic: string
  payload: Record<string, unknown>
}

export interface HassProcessResponse {
  message: string
  device: HassDiscoveredDevice
}

export interface HassRegisterResponse {
  message: string
  device_id: string
  device_type: string
  name: string
}

// Device Telemetry Types
export interface TelemetryDataResponse {
  device_id: string
  metrics: string[]
  data: Record<string, TelemetryPoint[]>
  start: number
  end: number
  aggregated: boolean
}

export interface TelemetryPoint {
  timestamp: number
  value: number | string | boolean | null
}

export interface TelemetrySummaryResponse {
  device_id: string
  summary: Record<string, TelemetryMetricSummary>
  start: number
  end: number
}

export interface TelemetryMetricSummary {
  display_name: string
  unit: string
  data_type: 'integer' | 'float' | 'string' | 'boolean' | 'binary'
  current: number | string | boolean | null
  current_timestamp: number | null
  avg: number | null
  min: number | null
  max: number | null
  count: number
}

export interface CommandHistoryResponse {
  device_id: string
  commands: CommandHistoryEntry[]
  count: number
  note?: string
}

export interface CommandHistoryEntry {
  timestamp: number
  command: string
  parameters: Record<string, unknown>
  success: boolean
  error?: string
}

// ========== Command History Types ==========

export interface CommandDto {
  id: string
  device_id: string
  command: string
  params: Record<string, unknown>
  status: string
  priority: string
  source_type: string
  source_id: string
  created_at: number
  executed_at: number | null
  attempt: number
  result: CommandResultDto | null
}

export interface CommandResultDto {
  success: boolean
  message: string
  response_data: Record<string, unknown> | null
  completed_at: number
}

export interface CommandListResponse {
  commands: CommandDto[]
  count: number
  total: number
  offset: number
  limit: number
}

export interface CommandStatsResponse {
  stats: {
    total_commands: number
    cache_size: number
    by_status: Array<{ status: string; count: number }>
    queue: {
      total_queued: number
      by_priority: Array<{ priority: string; count: number }>
    }
  }
}

// ========== Decision Types ==========

export interface DecisionDto {
  id: string
  title: string
  description: string
  reasoning: string
  actions: DecisionActionDto[]
  confidence: number
  decision_type: string
  priority: string
  status: string
  created_at: number
  executed_at: number | null
  execution_result: ExecutionResultDto | null
}

export interface DecisionActionDto {
  id: string
  action_type: string
  description: string
  parameters: Record<string, unknown>
  required: boolean
}

export interface ExecutionResultDto {
  success: boolean
  actions_executed: number
  success_count: number
  failure_count: number
  error: string | null
  timestamp: number
}

export interface DecisionListResponse {
  decisions: DecisionDto[]
  count: number
}

export interface DecisionStatsResponse {
  stats: {
    total_count: number
    by_type: Record<string, number>
    by_priority: Record<string, number>
    by_status: Record<string, number>
    avg_confidence: number
    success_rate: number
  }
}

// ========== Chat Input Types ==========

export interface ChatMessage {
  id: string
  role: 'user' | 'assistant' | 'system'
  content: string
  timestamp: number
  thinking?: string
  isStreaming?: boolean
}

// ========== Voice Recognition Types ==========

export interface VoiceRecognitionState {
  isListening: boolean
  transcript: string
  isSupported: boolean
  error?: string
}

// ========== Rules Types ==========
// Must match backend RuleDto and RuleDetailDto (crates/api/src/handlers/rules.rs)

export interface Rule {
  id: string
  name: string
  description?: string
  enabled: boolean
  trigger_count: number
  // Backend sends ISO 8601 strings, frontend may also use number timestamps
  last_triggered?: string | number
  created_at: string | number  // ISO 8601 from backend, number in UI
  updated_at?: string | number
  // Condition and actions (only present in detailed view)
  condition?: RuleCondition
  actions?: RuleAction[]
  // DSL text (used in creation/update)
  dsl?: string
}

export interface RuleCondition {
  device_id: string
  metric: string
  operator: string
  threshold: number
}

export type RuleAction =
  | { type: 'Notify'; message: string }
  | { type: 'Execute'; device_id: string; command: string; params: Record<string, unknown> }
  | { type: 'Log'; level: string; message: string }

// ========== Workflows Types ==========
// Must match backend WorkflowDto (crates/api/src/handlers/workflows.rs)

export interface Workflow {
  id: string
  name: string
  description: string
  enabled: boolean
  status: string  // 'active' | 'paused' | 'disabled' | 'failed'
  step_count: number
  trigger_count: number
  // Backend sends ISO 8601 strings, frontend may also use number timestamps
  created_at: string | number
  updated_at: string | number
  // For detailed view (not from basic DTO)
  triggers?: WorkflowTrigger[]
  steps?: WorkflowStep[]
  // UI may expect this field
  execution_count?: number
}

export interface WorkflowTrigger {
  type: 'event' | 'schedule' | 'manual' | 'device_state'
  config: Record<string, unknown>
}

export interface WorkflowStep {
  id: string
  name: string
  type: 'command' | 'condition' | 'delay' | 'notification' | 'llm'
  config: Record<string, unknown>
  next_steps?: string[]
}

export interface WorkflowExecution {
  id: string
  workflow_id: string
  status: 'pending' | 'running' | 'completed' | 'failed' | 'cancelled'
  started_at: number
  completed_at?: number
  input: Record<string, unknown>
  output?: Record<string, unknown>
  error?: string
}

// ========== Scenarios Types ==========

export interface Scenario {
  id: string
  name: string
  description?: string
  icon?: string
  actions: ScenarioAction[]
  enabled: boolean
  active: boolean
  created_at: number
  updated_at: number
}

export interface ScenarioAction {
  type: 'device_command' | 'scene' | 'delay' | 'notification'
  device_id?: string
  command?: string
  params?: Record<string, unknown>
  delay_ms?: number
}

export interface ScenarioTemplate {
  id: string
  name: string
  description: string
  icon: string
  category: string
  actions: Omit<ScenarioAction, 'device_id'>[]
}

// ========== Memory Types ==========

export interface MemoryEntry {
  id: string
  content: string
  importance: number
  created_at: number
  accessed_count?: number
  last_accessed?: number
  embedding?: number[]
}

export interface MemoryConsolidation {
  consolidated_count: number
  remaining_count: number
  message: string
}

// ========== Events Types ==========

export interface Event {
  id: string
  event_type: string
  source: string
  data: Record<string, unknown>
  timestamp: number
  processed: boolean
}

// ========== Plugins Types ==========
//
// Must match backend PluginDto and PluginStatsDto (crates/api/src/handlers/plugins.rs)
// and PluginType (crates/core/src/plugin/types.rs)

/**
 * Plugin type enumeration - matches backend PluginType
 * Uses snake_case strings to match Rust serde serialization
 */
export enum PluginTypeEnum {
  LlmBackend = 'llm_backend',
  StorageBackend = 'storage_backend',
  DeviceAdapter = 'device_adapter',
  InternalMqttBroker = 'internal_mqtt_broker',
  ExternalMqttBroker = 'external_mqtt_broker',
  HassDiscovery = 'hass_discovery',
  ModbusAdapter = 'modbus_adapter',
  Tool = 'tool',
  Integration = 'integration',
  AlertChannel = 'alert_channel',
  RuleEngine = 'rule_engine',
  WorkflowEngine = 'workflow_engine',
  Custom = 'custom',
}

/**
 * Plugin state enumeration - matches backend PluginState
 */
export enum PluginStateEnum {
  Loaded = 'Loaded',
  Initialized = 'Initialized',
  Running = 'Running',
  Stopped = 'Stopped',
  Error = 'Error',
  Paused = 'Paused',
}

/**
 * Plugin DTO - matches backend PluginDto exactly
 *
 * Backend: crates/api/src/handlers/plugins.rs:68-101
 */
export interface Plugin {
  id: string
  name: string
  plugin_type: string  // PluginType.as_str() - snake_case
  category: 'ai' | 'devices' | 'notify'  // User-facing category for UI grouping
  state: string  // PluginState - "Loaded", "Initialized", "Running", "Stopped", "Error(...)", "Paused"
  enabled: boolean
  version: string
  description: string
  author?: string
  required_version: string
  stats: PluginStatsDto
  loaded_at: string  // ISO 8601 DateTime<Utc>
  path?: string
  // Device adapter specific fields (when plugin_type === 'device_adapter')
  adapter_type?: 'mqtt' | 'modbus' | 'hass' | 'http' | 'custom'
  device_count?: number
  // Computed/Helper fields (not from backend, derived for UI)
  running?: boolean  // Derived from state === "Running"
}

/**
 * Plugin statistics DTO - matches backend PluginStatsDto exactly
 *
 * Backend: crates/api/src/handlers/plugins.rs:99-117
 */
export interface PluginStatsDto {
  start_count: number
  stop_count: number
  error_count: number
  total_execution_ms: number
  avg_response_time_ms: number
  last_start_time?: string
  last_stop_time?: string
}

// ========== Device Adapter Plugin Types ==========

export interface AdapterPluginDto {
  id: string
  name: string
  adapter_type: 'mqtt' | 'modbus' | 'hass' | 'http' | 'custom'
  enabled: boolean
  running: boolean
  device_count: number
  state: string
  version: string
  uptime_secs?: number
  last_activity: number
}

export interface AdapterDeviceDto {
  id: string
  name?: string
  device_type: string
  status: string
  last_seen: number
}

export interface AdapterDevicesResponse {
  plugin_id: string
  plugin_name: string
  devices: AdapterDeviceDto[]
  count: number
}

export interface DeviceAdapterPluginsResponse {
  total_adapters: number
  running_adapters: number
  total_devices: number
  adapters: AdapterPluginDto[]
}

export interface DeviceAdapterStats {
  total_adapters: number
  running_adapters: number
  total_devices: number
  adapters: AdapterPluginDto[]
}

// ========== Extended Device Types ==========

export interface Tool {
  name: string
  description?: string
  parameters?: Record<string, unknown>
  category?: string
}

export interface ToolSchema {
  name: string
  description: string
  parameters: {
    type: string
    properties: Record<string, { type: string; description: string }>
    required?: string[]
  }
}

export interface ToolMetrics {
  total_calls: number
  successful_calls: number
  failed_calls: number
  average_execution_time_ms: number
  by_tool: Record<string, { calls: number; avg_time_ms: number }>
}

export interface ToolExecutionResult {
  success: boolean
  result: unknown
  error?: string
  execution_time_ms: number
}

// ========== Search Types ==========

export interface SearchResult {
  type: 'device' | 'rule' | 'scenario' | 'workflow' | 'alert'
  id: string
  title: string
  description?: string
  relevance_score?: number
}

export interface SearchSuggestion {
  text: string
  type: string
  count: number
}

// ========== Extended Events Types ==========

export interface EventSubscription {
  id: string
  event_types: string[]
  source?: string
  created_at: number
}

export interface EventStats {
  total_events: number
  by_type: Record<string, number>
  by_source: Record<string, number>
  processed: number
  pending: number
}

// ========== LLM Backend Management Types ==========

export type LlmBackendType = 'ollama' | 'openai' | 'anthropic' | 'google' | 'xai'

export interface BackendCapabilities {
  supports_streaming: boolean
  supports_multimodal: boolean
  supports_thinking: boolean
  max_context: number
}

export interface LlmBackendInstance {
  id: string
  name: string
  backend_type: LlmBackendType
  endpoint?: string
  model: string
  api_key_configured: boolean
  is_active: boolean
  temperature: number
  top_p: number
  max_tokens: number
  capabilities: BackendCapabilities
  updated_at: number
  healthy?: boolean  // Health check result (from API)
}

export interface CreateLlmBackendRequest {
  name: string
  backend_type: LlmBackendType
  endpoint?: string
  model: string
  api_key?: string
  temperature?: number
  top_p?: number
  max_tokens?: number
}

export interface UpdateLlmBackendRequest {
  name?: string
  endpoint?: string
  model?: string
  api_key?: string
  temperature?: number
  top_p?: number
  max_tokens?: number
}

export interface LlmBackendListResponse {
  backends: LlmBackendInstance[]
  count: number
  active_id: string | null
}

export interface BackendTypeDefinition {
  id: string
  name: string
  description: string
  default_model: string
  default_endpoint?: string
  requires_api_key: boolean
  supports_streaming: boolean
  supports_thinking: boolean
  supports_multimodal: boolean
}

export interface BackendTestResult {
  success: boolean
  latency_ms?: number
  error?: string
}

export interface LlmBackendStats {
  total_backends: number
  active_backends: number
  by_type: Record<string, number>
  total_requests: number
  successful_requests: number
  failed_requests: number
  average_latency_ms: number
}

// ========== Plugin Config Schema Types ==========

export interface PluginConfigSchema {
  type: 'object'
  properties: Record<string, PropertySchema>
  required?: string[]
  ui_hints?: UiHints
}

export interface PropertySchema {
  type: 'string' | 'number' | 'boolean' | 'object' | 'array'
  description?: string
  default?: unknown
  enum?: unknown[]
  minimum?: number
  maximum?: number
  format?: string
  secret?: boolean  // For passwords, API keys
}

export interface UiHints {
  field_order?: string[]
  display_names?: Record<string, string>
  placeholders?: Record<string, string>
  help_texts?: Record<string, string>
  visibility_rules?: VisibilityRule[]
}

export interface VisibilityRule {
  field: string
  condition: 'equals' | 'not_equals' | 'contains' | 'empty' | 'not_empty'
  value?: unknown
  then_show: string[]
}

// ========== Device Adapter Plugin Types ==========
//
// Must match backend AdapterPluginDto (crates/api/src/handlers/plugins.rs:563-585)

export type DeviceAdapterType = 'mqtt' | 'modbus' | 'hass' | 'http' | 'custom'

/**
 * Adapter plugin DTO - matches backend AdapterPluginDto exactly
 *
 * Backend: crates/api/src/handlers/plugins.rs:563-585
 */
export interface AdapterPluginDto {
  id: string
  name: string
  adapter_type: DeviceAdapterType  // 'mqtt', 'modbus', 'hass', 'http', 'custom'
  enabled: boolean
  running: boolean
  device_count: number
  state: string
  version: string
  uptime_secs?: number
  last_activity: number
}

/**
 * Device adapter plugin with optional config (for UI)
 */
export interface DeviceAdapterPlugin extends AdapterPluginDto {
  config?: Record<string, unknown>
  config_schema?: PluginConfigSchema
}
