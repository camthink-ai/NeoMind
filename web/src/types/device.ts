// ========== Device Types ==========
// Device, connection, telemetry, commands, and auto-onboarding types.

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
  // Per-device offline-timeout override (seconds). Undefined = use template/global default.
  offline_timeout_secs?: number
  // Fully resolved offline timeout in effect (device override → template → global).
  // Read-only — lets the UI show "Default: Ns" without a separate API call.
  effective_offline_timeout_secs?: number
  // Transport-layer (MQTT session) connected flag — independent of `online`.
  // A device can be transport_connected=true but online=false if it has an
  // active MQTT session but hasn't published data within the offline timeout.
  transport_connected?: boolean
  // Epoch seconds of the last transport_connected state change. Undefined if
  // the broker never reported a transition (e.g. external broker w/o $SYS).
  transport_changed_at?: number
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
  // Origin marker: present for built-in seeded types (NE101/NE301/NE302…),
  // absent for user-created / imported ones
  builtin_version?: string | null
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

// MetricValue matches backend Rust enum serialization format
// The backend MetricValue enum serializes to direct JSON values:
// - MetricValue::Integer(42) -> 42
// - MetricValue::Float(3.14) -> 3.14
// - MetricValue::String("hello") -> "hello"
// - MetricValue::Boolean(true) -> true
// - MetricValue::Array([...]) -> [...]
// - MetricValue::Null -> null
export type MetricValue =
  | number    // Integer or Float (backend discriminates via JSON number type)
  | string    // String
  | boolean   // Boolean
  | MetricValue[]  // Array (heterogeneous arrays supported)
  | null      // Null

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

// Request to add a new device (updated for new architecture)
export interface AddDeviceRequest {
  device_id?: string
  name: string
  device_type: string  // Must reference an existing template
  adapter_type: string  // "mqtt"
  connection_config: ConnectionConfig
  /** Per-device offline-timeout override (seconds). null clears the override. */
  offline_timeout_secs?: number | null
}

export interface SendCommandRequest {
  params: Record<string, unknown>
}

// Device Telemetry Types
export interface TelemetryDataResponse {
  device_id: string
  metrics: string[]
  data: Record<string, TelemetryPoint[]>
  start: number
  end: number
  aggregated: boolean
  pagination?: {
    offset: number
    limit: number
    total: number
  }
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
    parameter_groups?: ParameterGroup[]
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

// ========== Auto-onboarding Types ==========

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
