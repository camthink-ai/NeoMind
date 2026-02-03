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
  name: string
  device_type: string  // Reference to template
  adapter_type: string  // "mqtt"
  connection_config?: ConnectionConfig  // Optional - only in detail view
  status: string
  last_seen: string
  online: boolean
  current_values?: Record<string, unknown>
  // Associated plugin information
  plugin_id?: string
  plugin_name?: string
  adapter_id?: string
  // Metric and command counts (from template)
  metric_count?: number
  command_count?: number
  // Legacy fields for backward compatibility
  config?: Record<string, string>
}

export interface ConnectionConfig {
  // MQTT-specific
  telemetry_topic?: string
  command_topic?: string
  json_path?: string
  // HTTP-specific
  url?: string
  method?: string
  poll_interval?: number
  headers?: Record<string, string>
  data_path?: string
  content_type?: string
  timeout?: number
  // Additional protocol-specific parameters
  [key: string]: unknown
}

export type DeviceTypeMode = 'simple' | 'full'

export interface DeviceType {
  device_type: string
  name: string
  description: string
  categories: string[]
  // Mode: simple (raw data + LLM) or full (structured definitions)
  mode?: DeviceTypeMode
  // Simplified: directly list metrics and commands, no uplink/downlink nesting
  // Optional - only included when fetching full details
  metrics?: MetricDefinition[]
  commands?: CommandDefinition[]
  // Samples for Simple mode - raw data examples
  uplink_samples?: Record<string, unknown>[]  // Sample uplink data
  // Optional counts for display purposes
  metric_count?: number
  command_count?: number
}

export interface MetricDefinition {
  name: string
  display_name: string
  data_type: 'integer' | 'float' | 'string' | 'boolean' | 'binary' | 'array'
  unit?: string
  min?: number
  max?: number
  required?: boolean
  // For array types: optional element type hint
  element_type?: 'integer' | 'float' | 'string' | 'boolean' | 'array'
  // Legacy fields for backward compatibility
  topic?: string
  value_template?: string
}

export interface CommandDefinition {
  name: string
  display_name: string
  payload_template?: string  // Template string, supports ${param} variables
  parameters?: ParameterDefinition[]
  // Fixed values - parameters that are always sent with the same value
  // These are not visible to users and are automatically included
  fixed_values?: Record<string, unknown>
  // Sample command payloads (for Simple mode / LLM reference)
  samples?: Record<string, unknown>[]
  // LLM hints for command usage
  llm_hints?: string
  // Parameter groups for organizing related parameters
  parameter_groups?: ParameterGroup[]
  // Legacy fields for backward compatibility
  topic?: string
  response_topic?: string
  timeout_ms?: number
}

export interface ParameterDefinition {
  name: string
  display_name?: string
  data_type: ParameterDataType
  default_value?: MetricValue
  min?: number
  max?: number
  unit?: string
  allowed_values?: MetricValue[]
  // Whether this parameter is required
  required?: boolean
  // Conditional visibility - show this parameter only when condition is met
  // Example: "mode == 'advanced'" or "brightness > 50"
  visible_when?: string
  // Parameter group for organizing related parameters
  group?: string
  // Help text for this parameter
  help_text?: string
  // Validation rules
  validation?: ValidationRule[]
}

// Parameter data type matching backend MetricDataType
export type ParameterDataType =
  | 'integer'
  | 'float'
  | 'string'
  | 'boolean'
  | 'binary'
  | 'array'
  | { enum: string[] }

// Validation rule for parameter values
export type ValidationRule =
  | { type: 'pattern'; regex: string; error_message: string }
  | { type: 'range'; min: number; max: number; error_message: string }
  | { type: 'length'; min: number; max: number; error_message: string }
  | { type: 'custom'; validator: string; params: Record<string, unknown> }

// Parameter group for organizing parameters in the UI
export interface ParameterGroup {
  id: string
  display_name: string
  description?: string
  collapsed?: boolean
  parameters: string[]
  order?: number
}

export type MetricValue =
  | { Integer: number }
  | { Float: number }
  | { String: string }
  | { Boolean: boolean }
  | { Array: MetricValue[] }
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

// ========== Message Types ==========
// New unified message/notification system

export type MessageSeverity = 'info' | 'warning' | 'critical' | 'emergency'
export type MessageStatus = 'active' | 'acknowledged' | 'resolved' | 'archived'
export type MessageCategory = 'alert' | 'system' | 'business'

/**
 * Message/Notification type - must match backend Message (crates/messages/src/message.rs)
 */
export interface NotificationMessage {
  id: string
  category: MessageCategory
  severity: MessageSeverity
  title: string
  message: string
  source: string
  source_type: string
  timestamp: string  // ISO 8601 string from backend
  status: MessageStatus
  metadata?: Record<string, unknown>
  tags: string[]
}

/**
 * Message list response
 */
export interface MessageListResponse {
  messages: NotificationMessage[]
  count: number
}

/**
 * Message statistics
 */
export interface MessageStats {
  total: number
  active: number
  by_category: Record<string, number>
  by_severity: Record<string, number>
  by_status: Record<string, number>
}

/**
 * Create message request
 */
export interface CreateMessageRequest {
  category: MessageCategory
  severity: MessageSeverity
  title: string
  message: string
  source?: string
  source_type?: string
  metadata?: Record<string, unknown>
  tags?: string[]
}

/**
 * Bulk message operation request
 */
export interface BulkMessageRequest {
  message_ids: string[]
}

/**
 * Cleanup old messages request
 */
export interface CleanupMessagesRequest {
  older_than_days: number
}

// Message Channel Types (formerly AlertChannel for backward compatibility)
export interface AlertChannel {
  name: string
  channel_type: 'console' | 'memory' | 'webhook' | 'email'
  enabled: boolean
  config?: Record<string, unknown>
}

export interface ChannelTypeInfo {
  id: string
  name: string
  name_zh: string
  description: string
  description_zh: string
  icon: string
  category: string
}

export interface ChannelStats {
  total: number
  enabled: number
  disabled: number
  by_type: Record<string, number>
}

export interface ChannelListResponse {
  channels: AlertChannel[]
  count: number
  stats: ChannelStats
}

export interface ChannelTestResult {
  success: boolean
  message: string
  message_zh: string
  duration_ms: number
}

export interface CreateChannelRequest {
  name: string
  channel_type: string
  [key: string]: unknown  // Additional config fields
}

export interface ChannelSchemaResponse {
  id: string
  name: string
  name_zh: string
  description: string
  description_zh: string
  icon: string
  category: string
  config_schema: JsonSchema
}

// ========== Message Channel Types ==========
// For the new unified messages system

export interface MessageChannel {
  name: string
  channel_type: 'console' | 'memory' | 'webhook' | 'email'
  enabled: boolean
  config?: Record<string, unknown>
}

export interface MessageChannelListResponse {
  channels: MessageChannel[]
  count: number
  stats: ChannelStats
}

export interface CreateMessageChannelRequest {
  name: string
  channel_type: string
  [key: string]: unknown  // Additional config fields
}

export type JsonSchema = {
  type?: string
  title?: string
  title_zh?: string
  description?: string
  description_zh?: string
  properties?: Record<string, JsonSchema>
  required?: string[]
  default?: unknown
  enum?: unknown[]
  minimum?: number
  maximum?: number
  format?: string
  x_secret?: boolean
  ui_hints?: {
    field_order?: string[]
    display_names?: Record<string, string>
    placeholders?: Record<string, string>
    help_texts?: Record<string, string>
    visibility_rules?: Array<{
      field: string
      condition: string
      value: unknown
      then_show?: string[]
      then_hide?: string[]
      then_disable?: string[]
    }>
  }
  items?: JsonSchema
  additionalProperties?: boolean | JsonSchema
}

// Chat Types
export interface Message {
  id: string
  role: 'user' | 'assistant' | 'system' | 'tool'
  content: string
  timestamp: number
  thinking?: string
  tool_calls?: ToolCall[]
  images?: ChatImage[]  // Images sent with user messages (multimodal)
  // Indicates if this message is still being streamed (partial)
  isPartial?: boolean
}

/**
 * All possible message roles including internal tool messages
 */
export type MessageRole = Message['role']

/**
 * Type guard to check if a message is a tool message (internal LLM context).
 * Tool messages should be filtered out from display.
 */
export function isToolMessage(msg: Message): msg is Message & { role: 'tool' } {
  return msg.role === 'tool'
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
  // Progress event during long-running operations
  // Note: stage is optional and can be any string (collecting, analyzing, executing, etc.)
  | { type: 'Progress'; elapsed: number; stage?: string; message?: string; remainingTime?: number; sessionId: string }
  // Warning event when approaching timeout
  | { type: 'Warning'; message: string; elapsed?: number; remainingTime?: number; sessionId: string }
  // Intent classification result (informational, not displayed in current UI)
  | { type: 'Intent'; category: string; displayName: string; confidence?: number; keywords?: string[]; sessionId: string }
  // Execution plan step (informational, not displayed in current UI)
  | { type: 'Plan'; step: string; stage: string; sessionId: string }
  // Heartbeat to keep connection alive (not displayed)
  | { type: 'Heartbeat'; timestamp: number; sessionId: string }
  // Error occurred - sessionId is always included when sent from backend
  | { type: 'Error'; message: string; sessionId: string }
  // Stream ended
  | { type: 'end'; sessionId: string }
  // Non-streaming response (fallback)
  | { type: 'response'; content: string; sessionId: string; toolsUsed?: string[]; processingTimeMs?: number }
  // Device status update
  | { type: 'device_update'; updateType: string; deviceId: string; status?: string; lastSeen?: number }

// Stream configuration types (matching backend StreamConfig)
export interface StreamConfig {
  maxThinkingChars: number
  maxThinkingTimeSecs: number
  maxStreamDurationSecs: number
  warningThresholds: number[]
  maxThinkingLoop: number
  progressEnabled: boolean
}

// Stream progress state for UI
export interface StreamProgress {
  elapsed: number
  stage?: string  // Optional stage from backend (collecting, analyzing, executing, etc.)
  warnings: string[]
  remainingTime: number
}

// Image data for multimodal messages
export interface ChatImage {
  data: string  // Base64 data URL (e.g., "data:image/png;base64,...")
  mimeType?: string  // e.g., "image/png", "image/jpeg"
}

// Client WebSocket message types
export interface ClientChatMessage {
  message: string
  images?: ChatImage[]  // Optional images for multimodal models
  sessionId?: string
  backendId?: string  // Optional LLM backend ID to use for this message
}

// MQTT Broker Types
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
  subscribe_topics?: string[]
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
  subscribe_topics?: string[]
}

export interface ApiResponse<T> {
  success?: boolean
  data?: T
  error?: string
  message?: string
}

// Request Types
// Request to add a new device (updated for new architecture)
export interface AddDeviceRequest {
  device_id?: string
  name: string
  device_type: string  // Must reference an existing template
  adapter_type: string  // "mqtt"
  connection_config: ConnectionConfig
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
  value: number | string | boolean | null | unknown[]
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
  data_type: 'integer' | 'float' | 'string' | 'boolean' | 'binary' | 'array'
  is_virtual?: boolean  // True for Transform-generated virtual metrics
  current: number | string | boolean | null | unknown[]
  current_timestamp: number | null
  avg: number | null
  min: number | null
  max: number | null
  count: number
}

// Unified device current state response
// Used by GET /api/devices/:id/current - returns device info + all metrics in one call
export interface DeviceCurrentStateResponse {
  device: {
    id: string
    device_id: string
    name: string
    device_type: string
    adapter_type: string
    status: string
    last_seen: string
    online: boolean
    plugin_id?: string
    plugin_name?: string
    adapter_id?: string
  }
  metrics: Record<string, DeviceMetricValue>
  commands: Array<{
    name: string
    display_name: string
    parameters?: ParameterDefinition[]
  }>
}

export interface DeviceMetricValue {
  name: string
  display_name: string
  unit: string
  data_type: 'integer' | 'float' | 'string' | 'boolean' | 'binary' | 'array'
  value: number | string | boolean | null | unknown[]
  is_virtual: boolean
}

// Batch current values response
// Used by POST /api/devices/current-batch - efficiently fetches current values for multiple devices
export interface BatchCurrentValuesResponse {
  devices: Record<string, {
    device_id: string
    current_values: Record<string, unknown>
  }>
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
  // Trigger (for unified automation system)
  trigger?: RuleTrigger
  // DSL text (used in creation/update)
  dsl?: string
  // Frontend-only source field for storing original UI state (not sent to backend)
  source?: {
    condition?: RuleCondition
    uiCondition?: any  // UICondition type from SimpleRuleBuilderSplit
    uiActions?: RuleAction[]
    forDuration?: number
    forUnit?: 'seconds' | 'minutes' | 'hours'
  }
}

export type RuleTrigger =
  | { type: 'device_state'; device_id: string; state: string }
  | { type: 'schedule'; cron: string }
  | { type: 'manual' }
  | { type: 'event'; event_type: string; filters?: Record<string, unknown> }

// Rule condition - supports simple, range, and logical (AND/OR/NOT) conditions
export interface RuleCondition {
  // Simple condition properties
  device_id?: string
  metric?: string
  operator?: string
  threshold?: number | string  // Supports numeric and string values for matching

  // Range condition properties
  range_min?: number

  // Logical condition properties
  conditions?: RuleCondition[]
}

export type RuleAction =
  | { type: 'Notify'; message: string }
  | { type: 'Execute'; device_id: string; command: string; params: Record<string, unknown> }
  | { type: 'Log'; level: string; message: string }
  | { type: 'Set'; device_id: string; property: string; value: unknown }
  | { type: 'Delay'; duration: number }
  | { type: 'CreateAlert'; title: string; message: string; severity: 'info' | 'warning' | 'error' | 'critical' }
  | { type: 'HttpRequest'; method: 'GET' | 'POST' | 'PUT' | 'DELETE' | 'PATCH'; url: string }

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
// DEPRECATED: Old Plugin system has been migrated to Extension system.
// See Extension Types below for the new API.
// Legacy plugin types are kept for backward compatibility with device adapters
// which still use /api/plugins/device-adapters endpoints.

/**
 * Plugin type enumeration - DEPRECATED, kept for device adapter compatibility
 */
export enum PluginTypeEnum {
  LlmBackend = 'llm_backend',
  StorageBackend = 'storage_backend',
  DeviceAdapter = 'device_adapter',
  InternalMqttBroker = 'internal_mqtt_broker',
  ExternalMqttBroker = 'external_mqtt_broker',
  Tool = 'tool',
  Integration = 'integration',
  AlertChannel = 'alert_channel',
  RuleEngine = 'rule_engine',
  Custom = 'custom',
}

/**
 * Plugin state enumeration - DEPRECATED
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
 * Plugin DTO - DEPRECATED, use Extension instead
 *
 * Only device adapters still use this interface.
 */
export interface Plugin {
  id: string
  name: string
  plugin_type: string
  category: 'ai' | 'devices' | 'notify'
  state: string
  enabled: boolean
  version: string
  description: string
  author?: string
  required_version: string
  stats: PluginStatsDto
  loaded_at: string
  path?: string
  adapter_type?: 'mqtt' | 'http' | 'custom'
  device_count?: number
  running?: boolean
}

/**
 * Plugin statistics DTO - DEPRECATED
 */
export interface PluginStatsDto {
  start_count: number
  stop_count: number
  error_count: number
  total_execution_ms: number
  avg_response_time_ms: number
  last_start_time?: string
  last_stop_time?: string
  device_count?: number
}

// ========== Extension Types ==========
//
// Matches backend ExtensionDto, ExtensionStatsDto, ExtensionTypeDto
// Backend: crates/api/src/handlers/extensions.rs
// And ExtensionType in crates/core/src/extension/types.rs

/**
 * Extension type enumeration - matches backend ExtensionType
 */
export enum ExtensionTypeEnum {
  LlmProvider = 'llm_provider',
  DeviceProtocol = 'device_protocol',
  AlertChannelType = 'alert_channel_type',
  Tool = 'tool',
  Generic = 'generic',
}

/**
 * Extension state enumeration - matches backend ExtensionState
 */
export enum ExtensionStateEnum {
  Discovered = 'Discovered',
  Loaded = 'Loaded',
  Initialized = 'Initialized',
  Running = 'Running',
  Stopped = 'Stopped',
  Error = 'Error',
}

/**
 * Extension DTO - matches backend ExtensionDto exactly
 *
 * Backend: crates/api/src/handlers/extensions.rs:18-39
 */
export interface Extension {
  id: string
  name: string
  extension_type: string  // ExtensionType.as_str() - snake_case
  version: string
  description?: string
  author?: string
  state: string  // ExtensionState - "Discovered", "Loaded", "Initialized", "Running", "Stopped", "Error"
  file_path?: string
  loaded_at?: number  // Unix timestamp
}

/**
 * Extension statistics DTO - matches backend ExtensionStatsDto exactly
 *
 * Backend: crates/api/src/handlers/extensions.rs:42-52
 */
export interface ExtensionStatsDto {
  start_count: number
  stop_count: number
  error_count: number
  last_error?: string
}

/**
 * Extension type DTO - matches backend ExtensionTypeDto exactly
 *
 * Backend: crates/api/src/handlers/extensions.rs:55-63
 */
export interface ExtensionTypeDto {
  id: string
  name: string
  description: string
}

/**
 * Extension discovery result - returned by POST /api/extensions/discover
 */
export interface ExtensionDiscoveryResult {
  id: string
  name: string
  version: string
  extension_type: string
  file_path?: string
}

/**
 * Extension registration response - returned by POST /api/extensions
 */
export interface ExtensionRegistrationResponse {
  message: string
  extension_id: string
  name: string
  version: string
  extension_type: string
  note?: string
}

/**
 * Extension health check response - returned by GET /api/extensions/:id/health
 */
export interface ExtensionHealthResponse {
  extension_id: string
  healthy: boolean
}

// ========== Device Adapter Plugin Types ==========

export interface AdapterPluginDto {
  id: string
  name: string
  adapter_type: 'mqtt' | 'http' | 'custom'
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
  type: 'device' | 'rule' | 'alert'
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

// ========== LLM Backend Management Types ==========

export type LlmBackendType = 'ollama' | 'openai' | 'anthropic' | 'google' | 'xai'

export interface BackendCapabilities {
  supports_streaming: boolean
  supports_multimodal: boolean
  supports_thinking: boolean
  supports_tools: boolean
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
  top_k: number
  max_tokens: number
  thinking_enabled: boolean  // Enable thinking/reasoning mode for models that support it
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
  top_k?: number
  thinking_enabled?: boolean  // Enable thinking/reasoning mode for models that support it
  capabilities?: BackendCapabilities  // Model capabilities (from Ollama model detection)
}

export interface UpdateLlmBackendRequest {
  name?: string
  endpoint?: string
  model?: string
  api_key?: string
  temperature?: number
  top_p?: number
  top_k?: number
  thinking_enabled?: boolean  // Enable thinking/reasoning mode for models that support it
  capabilities?: BackendCapabilities  // Model capabilities (from Ollama model detection)
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
  config_schema?: PluginConfigSchema
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

// ========== Device Adapter Types ==========
// Similar to LLM backend types, device adapters are now dynamically loaded

export interface AdapterType {
  id: string  // e.g., "mqtt", "http", "webhook"
  name: string  // e.g., "MQTT", "HTTP (Polling)", "Webhook"
  description: string
  icon: string  // Icon name for lucide-react
  icon_bg: string  // Tailwind CSS classes for icon background
  mode: 'push' | 'pull' | 'hybrid'  // Connection mode
  can_add_multiple: boolean  // Whether multiple instances can be created
  builtin: boolean  // Whether this is a built-in adapter
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

export type DeviceAdapterType = 'mqtt' | 'http' | 'custom'

/**
 * Adapter plugin DTO - matches backend AdapterPluginDto exactly
 *
 * Backend: crates/api/src/handlers/plugins.rs:563-585
 */
export interface AdapterPluginDto {
  id: string
  name: string
  adapter_type: DeviceAdapterType  // 'mqtt', 'http', 'custom'
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

// ========== Unified Automation Types ==========
// Must match backend AutomationDto and related types (crates/api/src/handlers/automations.rs)

/**
 * Automation type enumeration
 */
export type AutomationType = 'transform' | 'rule'

/**
 * Transform scope - determines what data the transform applies to
 * Matches backend Rust enum format with serde(rename_all = "snake_case")
 * - Global: "global" (string)
 * - DeviceType: { device_type: string }
 * - Device: { device: string }
 */
export type TransformScope =
  | 'global'
  | { device_type: string }
  | { device: string }

/**
 * Aggregation function for transforms
 */
export type AggregationFunc =
  | 'mean'
  | 'max'
  | 'min'
  | 'sum'
  | 'count'
  | 'first'
  | 'last'
  | 'median'
  | 'stddev'
  | 'trend'
  | 'delta'
  | 'rate'

/**
 * Time window for time-series aggregation
 */
export interface TimeWindow {
  duration_secs: number
  aggregation: AggregationFunc
}

/**
 * Data type for value conversion
 */
export type DataType = 'string' | 'number' | 'boolean' | 'int' | 'float'

/**
 * Data decode/encode format
 */
export type DecodeFormat = 'hex' | 'base64' | 'bytes' | 'csv' | 'url'

/**
 * Transform operation types
 *
 * Expression-based operations that AI can understand and generate:
 * - Extract: Get data using JSONPath
 * - Map: Transform array elements using template (e.g., convert nested arrays to "cls:fish num:12")
 * - Reduce: Aggregate array to single value
 * - Format: Template string formatting
 * - Compute: Mathematical expressions
 * - Pipeline: Chain operations
 * - Fork: Parallel branches
 * - If: Conditional execution
 * - GroupBy: Group array by key and aggregate (e.g., [{box, cls}] → count by cls)
 * - Decode: Convert encoded data (hex/base64) to JSON
 * - Encode: Convert JSON to encoded format
 */
export type TransformOperation =
  // Legacy operations (for backward compatibility)
  | { op_type: 'single'; json_path: string; output_metric: string }
  | { op_type: 'array_aggregation'; json_path: string; aggregation: AggregationFunc; value_path?: string; output_metric: string }
  | { op_type: 'time_series_aggregation'; source_metric: string; window: TimeWindow; output_metric: string }
  | { op_type: 'reference'; source_device: string; source_metric: string; output_metric: string }
  | { op_type: 'custom'; wasm_module_id: string; function_name: string; parameters: Record<string, unknown>; output_metrics: string[] }
  | { op_type: 'multi_output'; operations: TransformOperation[] }
  // Expression-based operations
  | { op_type: 'extract'; from: string; output: string; as_type?: DataType }
  | { op_type: 'map'; over: string; template: string; output: string; where?: string }
  | { op_type: 'reduce'; over: string; using: AggregationFunc; value?: string; output: string }
  | { op_type: 'format'; template: string; output: string; from?: string }
  | { op_type: 'compute'; expression: string; output: string }
  | { op_type: 'pipeline'; steps: TransformOperation[]; output: string }
  | { op_type: 'fork'; branches: TransformOperation[] }
  | { op_type: 'if'; condition: string; then: TransformOperation; else_?: TransformOperation; output: string }
  // Advanced data processing operations
  | { op_type: 'group_by'; over: string; key: string; using: AggregationFunc; value?: string; output: string }
  | { op_type: 'decode'; from: string; format: DecodeFormat; output: string }
  | { op_type: 'encode'; from: string; format: DecodeFormat; output: string }

/**
 * Transform automation - data processing layer
 *
 * New AI-Native Design:
 * - intent: User's natural language description
 * - js_code: AI-generated JavaScript transformation code
 * - output_prefix: Prefix for generated metric names
 */
export interface TransformAutomation extends BaseAutomation {
  type: 'transform'
  scope: TransformScope
  device_type_filter?: string

  // New AI-Native fields
  intent?: string // User's natural language intent
  js_code?: string // AI-generated JavaScript code
  output_prefix: string // Prefix for output metrics (default: "transform")
  complexity: number // 1-5, for execution ordering

  // Legacy field (deprecated, use js_code instead)
  operations?: TransformOperation[]
}

/**
 * Unified Automation type - can be Transform or Rule
 */
export type Automation = TransformAutomation | RuleAutomation

/**
 * Base automation interface with common fields
 */
export interface BaseAutomation {
  id: string
  name: string
  description: string
  enabled: boolean
  type: AutomationType
  created_at: number
  updated_at: number
  execution_count: number
  last_executed: number | null
}

/**
 * Rule automation - simple if-then conditions
 */
export interface RuleAutomation extends BaseAutomation {
  type: 'rule'
  trigger: RuleTrigger
  condition: RuleCondition
  actions: RuleAction[]
  complexity: number // 1-5
}

/**
 * Automation list response
 */
export interface AutomationListResponse {
  automations: Automation[]
  count: number
}

/**
 * Intent analysis result from AI
 */
export interface IntentResult {
  recommended_type: AutomationType
  confidence: number // 0-100
  reasoning: string
  suggested_automation: Partial<Automation> | null
  warnings: string[]
}

/**
 * Conversion recommendation
 */
export interface ConversionRecommendation {
  can_convert: boolean
  target_type: AutomationType
  reason: string
  estimated_complexity: number
}

/**
 * Create automation request
 */
export interface CreateAutomationRequest {
  name: string
  description?: string
  type?: AutomationType
  enabled?: boolean
  definition: Record<string, unknown>
}

/**
 * Update automation request
 */
export interface UpdateAutomationRequest {
  name?: string
  description?: string
  definition?: Record<string, unknown>
  enabled?: boolean
}

/**
 * Set automation status request
 */
export interface SetAutomationStatusRequest {
  enabled: boolean
}

/**
 * Convert automation request
 */
export interface ConvertAutomationRequest {
  type: AutomationType
}

/**
 * Automation execution record
 */
export interface ExecutionRecord {
  id: string
  automation_id: string
  started_at: number
  completed_at: number | null
  status: 'running' | 'completed' | 'failed' | 'cancelled'
  result?: Record<string, unknown>
  error?: string
}

/**
 * Automation template
 */
export interface AutomationTemplate {
  id: string
  name: string
  description: string
  automation_type: AutomationType
  category: string
  parameters: TemplateParameter[]
  definition_template: Record<string, unknown>
}

/**
 * Template parameter for automation templates
 */
export interface TemplateParameter {
  name: string
  label: string
  param_type: 'string' | 'number' | 'boolean' | 'device' | 'metric' | 'enum'
  default?: string | null
  required: boolean
  options: string[]
}

/**
 * Automation filter parameters
 */
export interface AutomationFilter {
  type?: 'transform' | 'rule' | 'all'
  enabled?: boolean
  search?: string
}

// ========== Auto-onboarding Types ==========

/**
 * Draft device from auto-discovery
 */
/**
 * Raw device data sample for analysis
 */
export interface DeviceSample {
  raw_data: number[]  // bytes as array
  parsed?: Record<string, unknown> | null  // parsed JSON if available
  source: string  // e.g., "MQTT: topic/sensor/001"
  timestamp: number
}

export interface DraftDevice {
  id: string
  device_id: string
  source: string
  status: string
  samples?: DeviceSample[]  // Original raw samples
  sample_count: number
  max_samples: number
  generated_type?: GeneratedDeviceType
  discovered_at: number
  updated_at: number
  error_message?: string
  user_name?: string
  user_description?: string
}

/**
 * Generated device type from auto-discovery
 */
export interface GeneratedDeviceType {
  device_type: string
  name: string
  description: string
  category: string
  metrics: MetricSummary[]
  confidence: number
  summary: ProcessingSummary
}

/**
 * Metric summary from auto-discovery
 */
export interface MetricSummary {
  name: string
  path: string
  semantic_type: string
  display_name: string
  confidence: number
  data_type?: string
  unit?: string
  description?: string
  is_readable?: boolean
  is_writable?: boolean
}

/**
 * Processing summary from auto-discovery
 */
export interface ProcessingSummary {
  samples_analyzed: number
  fields_discovered: number
  metrics_generated: number
  inferred_category: string
  insights: string[]
  warnings: string[]
  recommendations: string[]
}

/**
 * Suggested device type for a draft device
 */
export interface SuggestedDeviceType {
  device_type: string
  name: string
  description: string
  match_score: number
  is_exact_match: boolean
  metric_count: number
}

/**
 * New device type definition from draft
 */
export interface NewDeviceTypeDefinition {
  device_type: string
  name: string
  description: string
  categories: string[]
  metrics: Array<{
    name: string
    display_name: string
    data_type: string
    unit: string
    path?: string
  }>
  commands?: Array<{
    name: string
    display_name: string
    parameters?: Array<{
      name: string
      param_type: string
      required: boolean
      default_value?: unknown
    }>
  }>
}

// ========== AI Agent Types ==========
// Must match backend AgentDto and related types (crates/api/src/handlers/agents.rs)

/**
 * AI Agent status enumeration
 */
export type AgentStatus = 'Active' | 'Paused' | 'Error' | 'Executing'

/**
 * Schedule type for agent execution
 */
export type AgentScheduleType = 'interval' | 'cron' | 'event'

/**
 * Resource type for agent resources
 */
export type AgentResourceType = 'Device' | 'Metric' | 'Command'

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
  // Conversation history fields
  conversation_history: ConversationTurn[]
  user_messages: UserMessage[]
  conversation_summary: string | null
  context_window_size: number
}

/**
 * Agent schedule configuration
 */
export interface AgentSchedule {
  schedule_type: 'interval' | 'cron' | 'event'
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
export interface AgentMemory {
  // Hierarchical memory structure
  working: WorkingMemory
  short_term: ShortTermMemory
  long_term: LongTermMemory
  // Legacy fields (backward compatibility)
  state_variables: Record<string, unknown>
  baselines?: Record<string, number>
  learned_patterns: LearnedPattern[]
  trend_data: TrendPoint[]
  updated_at: string
}

/**
 * Working memory - current execution context
 */
export interface WorkingMemory {
  current_analysis: string | null
  current_conclusion: string | null
  created_at: string
}

/**
 * Short-term memory - recent summaries
 */
export interface ShortTermMemory {
  summaries: MemorySummary[]
  max_summaries: number
  last_archived_at: string | null
}

/**
 * Memory summary for short-term storage
 */
export interface MemorySummary {
  timestamp: string
  execution_id: string
  situation: string
  conclusion: string
  decisions: string[]
  success: boolean
}

/**
 * Long-term memory - important patterns
 */
export interface LongTermMemory {
  memories: ImportantMemory[]
  patterns: LearnedPattern[]
  max_memories: number
  min_importance: number
}

/**
 * Important memory for long-term storage
 */
export interface ImportantMemory {
  id: string
  memory_type: string
  content: string
  importance: number
  created_at: string
  access_count: number
}

/**
 * Learned pattern from historical data
 */
export interface LearnedPattern {
  id: string
  pattern_type: string
  description: string
  confidence: number
  learned_at: number  // Unix timestamp
  data: Record<string, unknown>
}

/**
 * Trend data point for memory
 */
export interface TrendPoint {
  timestamp: number
  metric: string
  value: number
  context?: Record<string, unknown>
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
 * Input for a single conversation turn (execution)
 */
export interface TurnInput {
  data_collected: DataCollected[]
  event_data: Record<string, unknown> | null
}

/**
 * Output from a single conversation turn (execution)
 */
export interface TurnOutput {
  situation_analysis: string
  reasoning_steps: ReasoningStep[]
  decisions: Decision[]
  conclusion: string
}

/**
 * A single conversation turn - one complete execution with context
 */
export interface ConversationTurn {
  execution_id: string
  timestamp: number
  trigger_type: string
  input: TurnInput
  output: TurnOutput
  duration_ms: number
  success: boolean
}

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
  device_ids: string[]
  metrics: MetricSelectionRequest[]
  commands: CommandSelectionRequest[]
  schedule: AgentScheduleRequest
  llm_backend_id?: string
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
 * Request to update an agent
 */
export interface UpdateAgentRequest {
  name?: string
  user_prompt?: string
  status?: string
}

/**
 * Request to execute an agent
 */
export interface ExecuteAgentRequest {
  trigger_type?: string
  event_data?: Record<string, unknown>
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

// ============================================================================
// Dashboard Types
// ============================================================================

/**
 * Dashboard response from API
 */
export interface DashboardResponse {
  id: string
  name: string
  layout: {
    columns: number
    rows: number | 'auto'
    breakpoints: {
      lg: number
      md: number
      sm: number
      xs: number
    }
  }
  components: DashboardComponentResponse[]
  created_at: number
  updated_at: number
  is_default?: boolean
}

/**
 * Dashboard component response from API
 */
export interface DashboardComponentResponse {
  id: string
  type: string
  position: {
    x: number
    y: number
    w: number
    h: number
    min_w?: number
    min_h?: number
    max_w?: number
    max_h?: number
  }
  title?: string
  config?: Record<string, unknown>
  data_source?: {
    type: string
    endpoint?: string
    transform?: string
    refresh?: number
    params?: Record<string, unknown>
    static_value?: unknown
  }
  display?: Record<string, unknown>
  actions?: Array<{
    type: string
    method?: string
    endpoint?: string
    path?: string
    dialog?: string
    confirm?: boolean
  }>
}

/**
 * Request to create a dashboard
 */
export interface CreateDashboardRequest {
  name: string
  layout: DashboardResponse['layout']
  components: Omit<DashboardComponentResponse, 'id'>[]
}

/**
 * Request to update a dashboard
 */
export interface UpdateDashboardRequest {
  name?: string
  layout?: DashboardResponse['layout']
  components?: DashboardComponentResponse[]
}

/**
 * Dashboard template response
 */
export interface DashboardTemplateResponse {
  id: string
  name: string
  description: string
  category: string
  icon?: string
  layout: DashboardResponse['layout']
  components: Omit<DashboardComponentResponse, 'id'>[]
  required_resources?: {
    devices?: number
    agents?: number
    rules?: number
  }
}


