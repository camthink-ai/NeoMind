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
  // Ping from server - client auto-responds with pong
  | { type: 'ping'; timestamp: number }
  // Error occurred - sessionId is always included when sent from backend
  | { type: 'Error'; message: string; sessionId: string }
  // Stream ended
  | { type: 'end'; sessionId: string }
  // Intermediate end for multi-round tool calling (indicates more content coming)
  | { type: 'intermediate_end'; sessionId: string }
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
  tags?: string[]  // Rule tags for categorization
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
    tags?: string[]  // Store tags in source for restoration
  }
}

export type RuleTrigger =
  | { type: 'device_state'; device_id: string; state: string }
  | { type: 'schedule'; cron: string }
  | { type: 'manual' }
  | { type: 'event'; event_type: string; filters?: Record<string, unknown> }

// Rule condition - supports simple, range, extension, and logical (AND/OR/NOT) conditions
export interface RuleCondition {
  // Condition type discriminator
  condition_type?: 'device' | 'extension' | 'logical' | 'range'

  // Device condition properties (when condition_type === 'device')
  device_id?: string
  metric?: string
  operator?: string
  threshold?: number | string  // Supports numeric and string values for matching

  // Extension condition properties (when condition_type === 'extension')
  extension_id?: string
  extension_metric?: string

  // Range condition properties
  range_min?: number
  range_max?: number

  // Logical condition properties
  logical_operator?: 'and' | 'or' | 'not'
  conditions?: RuleCondition[]
}

export type RuleAction =
  | { type: 'Notify'; message: string; channels?: string[] }
  | { type: 'Execute'; device_id: string; command: string; params: Record<string, unknown> }
  | { type: 'Log'; level: string; message: string; severity?: string }
  | { type: 'Set'; device_id: string; property: string; value: unknown }
  | { type: 'Delay'; duration: number }
  | { type: 'CreateAlert'; title: string; message: string; severity: 'info' | 'warning' | 'error' | 'critical' }
  | { type: 'HttpRequest'; method: 'GET' | 'POST' | 'PUT' | 'DELETE' | 'PATCH'; url: string; headers?: Record<string, string>; body?: string }

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
  auto_start?: boolean
}

/**
 * Extension health check response - returned by GET /api/extensions/:id/health
 */
export interface ExtensionHealthResponse {
  extension_id: string
  healthy: boolean
}

// ========== Extension Capability Types ==========
//
// These types describe the capabilities that extensions can provide

/**
 * Extension capability types - matches backend ExtensionCapability
 * Backend: crates/core/src/extension/types.rs
 */
export type ExtensionCapabilityType = 'tool' | 'provider' | 'processor' | 'notifier' | 'hybrid'

/**
 * Tool descriptor - describes a tool provided by an extension
 */
export interface ToolDescriptor {
  name: string
  description: string
  parameters: Record<string, unknown>  // JSON Schema for parameters
  returns?: string
}

/**
 * Metric descriptor - describes a metric provided by an extension
 */
export interface MetricDescriptor {
  name: string
  data_type: string
  unit?: string
  description?: string
}

/**
 * Channel descriptor - describes a notification channel provided by an extension
 */
export interface ChannelDescriptor {
  name: string
  display_name: string
  description: string
  config_schema: Record<string, unknown>
}

/**
 * Extension capability DTO - describes an extension's capabilities
 * Backend: crates/api/src/handlers/extensions.rs
 */
export interface ExtensionCapabilityDto {
  extension_id: string
  extension_name: string
  type: ExtensionCapabilityType

  // Tool capability
  tools?: ToolDescriptor[]

  // Provider capability
  metrics?: MetricDescriptor[]

  // Processor capability
  input_schema?: Record<string, unknown>
  output_schema?: Record<string, unknown>

  // Notifier capability
  channels?: ChannelDescriptor[]

  // Hybrid capability - nested capabilities
  capabilities?: ExtensionCapabilityDto[]

  // Commands for processor extensions
  commands?: Array<{
    name: string
    description: string
  }>
}

/**
 * Extension tool for UI display
 */
export interface ExtensionTool {
  extension_id: string
  extension_name: string
  tool_name: string
  description: string
  parameters: Record<string, unknown>
  returns?: string
}

/**
 * Extension transform operation for automation
 */
export interface ExtensionTransformOperation {
  extension_id: string
  command: string
  parameters: Record<string, unknown>
  output_metrics: string[]
}

/**
 * Extension data source for dashboard
 */
export interface ExtensionDataSource {
  extension_id: string
  metric_name: string
  display_name: string
  data_type: string
  unit?: string
}

// ========== Extension Types ==========
//
// Extension system - unified command-based approach
// - Metrics and commands are separate
// - Commands don't declare output fields

/**
 * Data type for extension metrics
 * Matches backend MetricDataType enum
 */
export type ExtensionDataType = 'integer' | 'number' | 'string' | 'boolean' | 'array' | 'object' | 'binary'

/**
 * Aggregation functions for extension metrics
 * Matches backend AggFunc enum
 */
export type ExtensionAggFunc = 'avg' | 'sum' | 'min' | 'max' | 'count' | 'last'

/**
 * Metric descriptor DTO - describes a metric provided by an extension
 */
export interface ExtensionMetricDto {
  name: string
  display_name: string
  data_type: string
  unit: string
  description?: string
  min?: number
  max?: number
  required: boolean
}

/**
 * Command descriptor
 * Matches backend ExtensionCommand struct
 */
export interface ExtensionCommandDescriptor {
  id: string  // Command name (from cmd.name)
  display_name: string
  description: string  // AI hints (from llm_hints)
  input_schema: Record<string, unknown>  // Built from parameters
  // output_fields removed - commands no longer declare output
  // config removed - no execution config
}

/**
 * Extension DTO
 * Matches backend ExtensionDto exactly
 */
export interface Extension {
  id: string
  name: string
  version: string
  description?: string
  author?: string
  state: string
  commands: ExtensionCommandDescriptor[]
  metrics: ExtensionMetricDto[]
  file_path?: string
  loaded_at?: number
}

/**
 * Extension command execution request
 */
export interface ExtensionExecuteRequest {
  command: string
  args?: Record<string, unknown>
}

/**
 * Extension command execution response
 */
export interface ExtensionExecuteResponse {
  success: boolean
  output: Record<string, unknown>
  outputs: Array<{
    name: string
    value: unknown
    unit?: string
    quality?: number
  }>
  duration_ms: number
  error?: string
}

/**
 * Extension configuration parameter definition
 */
export interface ExtensionConfigParameter {
  name: string
  title?: string
  description?: string
  type: 'string' | 'number' | 'integer' | 'boolean' | 'array'
  default?: unknown
  enum?: string[]
  minimum?: number
  maximum?: number
}

/**
 * Extension configuration schema
 */
export interface ExtensionConfigSchema {
  type: 'object'
  properties: Record<string, ExtensionConfigParameter>
  required: string[]
}

/**
 * Extension configuration response
 */
export interface ExtensionConfigResponse {
  extension_id: string
  extension_name: string
  config_schema: ExtensionConfigSchema
  current_config: Record<string, unknown>
}

/**
 * Data source info for query integration
 * Format: "extension:{extension_id}:{metric}"
 */
export interface ExtensionDataSourceInfo {
  id: string
  extension_id: string
  command: string
  field: string
  display_name: string
  data_type: ExtensionDataType
  unit?: string
  description: string
  aggregatable: boolean
  default_agg_func: ExtensionAggFunc
}

/**
 * Transform output data source info
 * Format: "transform:{transform_id}:{metric_name}"
 */
export interface TransformDataSourceInfo {
  id: string
  transform_id: string
  transform_name: string
  metric_name: string
  display_name: string
  data_type: string
  unit?: string
  description: string
  last_update?: number
}

/**
 * Unified query parameters
 */
export interface ExtensionQueryParams {
  extension_id: string
  command: string
  field: string
  start_time?: number
  end_time?: number
  aggregation?: ExtensionAggFunc
  limit?: number
}

/**
 * Query result
 */
export interface ExtensionQueryResult {
  source_id: string
  data_points: Array<{
    timestamp: number
    value: unknown
    quality?: number
  }>
  aggregation?: {
    func: ExtensionAggFunc
    value: unknown
  }
}

// Legacy type aliases for backward compatibility
export type ExtensionV2DataType = ExtensionDataType
export type ExtensionV2AggFunc = ExtensionAggFunc
export type ExtensionV2CommandDescriptor = ExtensionCommandDescriptor
export type ExtensionV2 = Extension
export type ExtensionV2ExecuteRequest = ExtensionExecuteRequest
export type ExtensionV2ExecuteResponse = ExtensionExecuteResponse
export type ExtensionV2DataSourceInfo = ExtensionDataSourceInfo
export type ExtensionV2QueryParams = ExtensionQueryParams
export type ExtensionV2QueryResult = ExtensionQueryResult

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
  config_schema?: Record<string, unknown>  // JSON Schema for configuration
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
  extension_operation?: ExtensionTransformOperation // Extension-based transform
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
  // Tool chaining configuration
  enable_tool_chaining?: boolean
  max_chain_depth?: number
  priority?: number
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
  device_ids?: string[]
  metrics?: MetricSelectionRequest[]
  commands?: CommandSelectionRequest[]
  resources?: ResourceRequest[]
  schedule: AgentScheduleRequest
  llm_backend_id?: string
  /** Enable tool chaining (default: false) */
  enable_tool_chaining?: boolean
  /** Maximum chain depth (default: 3) */
  max_chain_depth?: number
  /** Agent priority 1-10 (default: 5) */
  priority?: number
  /** Context window size in tokens (default: 8192) */
  context_window_size?: number
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
  enable_tool_chaining?: boolean
  /** Maximum chain depth (default: 3) */
  max_chain_depth?: number
  /** Agent priority 1-10 (default: 5) */
  priority?: number
  /** Context window size in tokens (default: 8192) */
  context_window_size?: number
}

/**
 * Request to execute an agent
 */
export interface ExecuteAgentRequest {
  trigger_type?: string
  event_data?: Record<string, unknown>
}

/**
 * Request to validate LLM backend
 */
export interface ValidateLlmRequest {
  backend_id?: string
  model?: string
  test_prompt?: string
}

/**
 * Response from LLM validation
 */
export interface ValidateLlmResponse {
  valid: boolean
  backend_name?: string
  model?: string
  error?: string
  response_time_ms?: number
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

// ============================================================================
// Extension Streaming Types
// ============================================================================
//
// Generic streaming support for extensions
// Supports: Image analysis, Video processing, Audio streaming, Sensor data, etc.

/**
 * Stream direction - matches backend StreamDirection enum
 */
export type StreamDirection = 'upload' | 'download' | 'bidirectional'

/**
 * Stream mode - matches backend StreamMode enum
 */
export type StreamMode = 'stateless' | 'stateful' | 'push'

/**
 * Stream data type - matches backend StreamDataType enum
 */
export type StreamDataTypeBase = 'binary' | 'text' | 'json'

export interface StreamDataTypeImage {
  type: 'image'
  format: string  // e.g., 'jpeg', 'png', 'webp'
}

export interface StreamDataTypeAudio {
  type: 'audio'
  format: string  // e.g., 'mp3', 'wav', 'aac'
  sample_rate: number
  channels: number
}

export interface StreamDataTypeVideo {
  type: 'video'
  codec: string  // e.g., 'h264', 'h265', 'vp9'
  width: number
  height: number
  fps: number
}

export interface StreamDataTypeSensor {
  type: 'sensor'
  sensor_type: string
}

export interface StreamDataTypeCustom {
  type: 'custom'
  mime_type: string
}

export type StreamDataType = StreamDataTypeBase | StreamDataTypeImage | StreamDataTypeAudio | StreamDataTypeVideo | StreamDataTypeSensor | StreamDataTypeCustom

/**
 * Flow control settings - matches backend FlowControl struct
 */
export interface FlowControl {
  supports_backpressure: boolean
  window_size: number
  supports_throttling: boolean
  max_rate: number
}

/**
 * Stream capability - matches backend StreamCapability struct
 */
export interface StreamCapability {
  direction: StreamDirection
  mode: StreamMode
  supported_data_types: StreamDataType[]
  max_chunk_size: number
  preferred_chunk_size: number
  max_concurrent_sessions: number
  flow_control: FlowControl
  config_schema?: Record<string, unknown>
}

/**
 * Client message types for extension streaming WebSocket
 */
export interface ExtensionClientMessageHello {
  type: 'hello'
}

export interface ExtensionClientMessageInit {
  type: 'init'
  config?: Record<string, unknown>
}

export interface ExtensionClientMessageAck {
  type: 'ack'
  sequence: number
}

export interface ExtensionClientMessageClose {
  type: 'close'
}

export type ExtensionClientMessage = ExtensionClientMessageHello | ExtensionClientMessageInit | ExtensionClientMessageAck | ExtensionClientMessageClose

/**
 * Server message types for extension streaming WebSocket
 */
export interface ExtensionServerMessageCapability {
  type: 'capability'
  capability: StreamCapability
}

export interface ExtensionServerMessageSessionCreated {
  type: 'session_created'
  session_id: string
  server_time: number
}

export interface ExtensionServerMessageResult {
  type: 'result'
  input_sequence: number | null
  output_sequence: number
  data: string  // base64 encoded
  data_type: string
  processing_ms: number
  metadata?: Record<string, unknown>
}

export interface ExtensionServerMessageError {
  type: 'error'
  code: string
  message: string
  retryable: boolean
}

export interface ExtensionServerMessageSessionClosed {
  type: 'session_closed'
  session_id: string
  total_frames: number
  duration_ms: number
  stats: ExtensionSessionStats
}

export interface ExtensionServerMessageHeartbeat {
  type: 'heartbeat'
  timestamp: number
}

export type ExtensionServerMessage = ExtensionServerMessageCapability | ExtensionServerMessageSessionCreated | ExtensionServerMessageResult | ExtensionServerMessageError | ExtensionServerMessageSessionClosed | ExtensionServerMessageHeartbeat

/**
 * Session statistics - matches backend SessionStats struct
 */
export interface ExtensionSessionStats {
  input_chunks: number
  output_chunks: number
  input_bytes: number
  output_bytes: number
  errors: number
  last_activity: number
}

/**
 * Stream connection state for extensions
 */
export interface ExtensionStreamConnectionState {
  status: 'connecting' | 'connected' | 'disconnected' | 'error'
  sessionId: string | null
  errorMessage?: string
  capability: StreamCapability | null
}

/**
 * Stream result - processed data from extension
 */
export interface ExtensionStreamResult {
  inputSequence: number | null
  outputSequence: number
  data: Uint8Array
  dataType: string
  processingMs: number
  metadata?: Record<string, unknown>
}

/**
 * Options for useExtensionStream hook
 */
export interface UseExtensionStreamOptions {
  /**
   * Extension ID to connect to
   */
  extensionId: string

  /**
   * Initial configuration for stateful sessions
   */
  config?: Record<string, unknown>

  /**
   * Enable auto-connect
   */
  enabled?: boolean

  /**
   * Result callback - receives processed data
   */
  onResult?: (result: ExtensionStreamResult) => void

  /**
   * Error callback
   */
  onError?: (error: Error) => void

  /**
   * Connection state change callback
   */
  onConnectionChange?: (state: ExtensionStreamConnectionState) => void

  /**
   * Session closed callback
   */
  onSessionClosed?: (stats: ExtensionSessionStats) => void
}

// ============================================================================
// Dashboard Components from Extensions
// ============================================================================

/**
 * Component category for dashboard widgets
 */
export type ComponentCategory =
  | 'indicators' // Value displays (cards, indicators)
  | 'charts' // Charts and graphs
  | 'controls' // Interactive inputs
  | 'display' // Content display
  | 'spatial' // Maps, video, layers
  | 'business' // Business-specific components
  | 'custom' // Extension-provided custom components

/**
 * Size constraints for dashboard components
 */
export interface SizeConstraints {
  min_w: number
  min_h: number
  default_w: number
  default_h: number
  max_w: number
  max_h: number
  preserve_aspect?: boolean
}

/**
 * Data binding configuration for extension components
 */
export interface DataBindingConfig {
  extension_metric?: string
  extension_command?: string
  required_fields: string[]
}

/**
 * Dashboard component DTO from extension
 */
export interface DashboardComponentDto {
  /** Component type identifier */
  type: string
  /** Display name */
  name: string
  /** Description */
  description: string
  /** Component category */
  category: ComponentCategory
  /** Icon name (lucide-react) */
  icon?: string
  /** Bundle URL (resolved) */
  bundle_url: string
  /** Export name in bundle */
  export_name: string
  /** Size constraints */
  size_constraints: SizeConstraints
  /** Whether this component accepts a data source */
  has_data_source: boolean
  /** Whether this component has display configuration */
  has_display_config: boolean
  /** Whether this component has actions */
  has_actions: boolean
  /** Maximum number of data sources */
  max_data_sources: number
  /** JSON Schema for component configuration */
  config_schema?: JSONSchema
  /** JSON Schema for data source binding */
  data_source_schema?: JSONSchema
  /** Default configuration values */
  default_config?: Record<string, unknown>
  /** Component variants */
  variants: string[]
  /** Data binding configuration */
  data_binding: DataBindingConfig
  /** Extension ID */
  extension_id: string
}

/**
 * Dashboard components list response
 */
export interface DashboardComponentsResponse {
  extension_id: string
  extension_name: string
  components: DashboardComponentDto[]
}

/**
 * JSON Schema definition
 */
export interface JSONSchema {
  type?: string
  properties?: Record<string, JSONSchema>
  required?: string[]
  items?: JSONSchema
  enum?: (string | number | boolean)[]
  default?: unknown
  description?: string
  title?: string
  minimum?: number
  maximum?: number
  [key: string]: unknown
}

/**
 * Plugin configuration schema (legacy - for backward compatibility)
 */
export interface PluginConfigSchema {
  type: 'object'
  properties: Record<string, {
    type: string
    description?: string
    enum?: string[]
    items?: any
    properties?: any
    required?: string[]
    default?: any
    minimum?: number
    maximum?: number
    format?: string
    secret?: boolean
    [key: string]: any
  }>
  required?: string[]
  ui_hints?: any
  [key: string]: any
}




