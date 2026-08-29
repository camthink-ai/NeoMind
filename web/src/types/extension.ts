// ========== Extension Types ==========
//
// Matches backend ExtensionDto, ExtensionTypeDto
// Backend: crates/api/src/handlers/extensions.rs
// And ExtensionType in crates/core/src/extension/types.rs

/**
 * Extension state enumeration - matches backend ExtensionState
 */
export enum ExtensionStateEnum {
  Running = 'Running',
  RunningIsolated = 'Running (Isolated)',
  Stopped = 'Stopped',
  Error = 'Error',
}

/**
 * Extension log entry - matches backend ExtensionLogEntryDto
 *
 * Backend: crates/api/src/handlers/extensions.rs
 */
export interface ExtensionLogEntry {
  timestamp: number
  level: string
  message: string
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

export interface ExtensionConfigParam {
  name: string
  display_name: string
  data_type: string
  unit: string
  description?: string
  min?: number
  max?: number
  required: boolean
  default_value?: unknown
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
  /** True when this command is hidden from the LLM tool registry. */
  disabled?: boolean
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
  /** Consecutive crashes (0 = healthy / stopped on purpose). */
  consecutive_crashes?: number
  /** Reason for the most recent crash, when known. */
  last_crash_reason?: string | null
  commands: ExtensionCommandDescriptor[]
  metrics: ExtensionMetricDto[]
  file_path?: string
  loaded_at?: number
  health_status: string
  last_error?: string
  last_error_at?: number
  config_parameters?: ExtensionConfigParam[]
  /** Master tool-toggle. When false, none of this extension's tools reach the LLM. */
  enabled?: boolean
  /** Per-command disable list (command ids without extension-id prefix). */
  disabled_commands?: string[]
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
 * Backend returns the extension's raw JSON result directly.
 */
export type ExtensionExecuteResponse = Record<string, unknown>

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
  propertyOrder?: string[]
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
 * Unified data source info from the /api/data/sources endpoint
 * Aggregates data sources from devices, extensions, and transforms
 */
export interface UnifiedDataSourceInfo {
  /** Unique identifier: "{type}:{source}:{field}" */
  id: string
  /** Source type: "device" | "extension" | "transform" | "system" */
  source_type: string
  /** Source name (device_id, extension_id, transform_id, or "system") */
  source_name: string
    /** Display name of the source */
  source_display_name: string
  /** Field/metric name */
  field: string
  /** Display name of the field */
  field_display_name: string

  /** Data type */
  data_type: string
  /** Unit of measurement */
  unit?: string
  /** Human-readable description */
  description?: string
  /** Current value (if available) */
  current_value?: unknown
  /** Last update timestamp (Unix milliseconds) */
  last_update?: number
  /** Data quality score (0.0 - 1.0) */
  quality?: number
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
