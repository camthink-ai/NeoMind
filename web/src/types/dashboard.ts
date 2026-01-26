/**
 * Dashboard type definitions for NeoTalk
 *
 * Two-layer component system:
 * - Generic Components: Reusable IoT/dashboard components
 * - Business Components: NeoTalk-specific components
 */

// ============================================================================
// Data Source Types
// ============================================================================

export type DataSourceType = 'api' | 'websocket' | 'static' | 'computed' | 'device' | 'metric' | 'command' | 'telemetry' | 'device-info'

export interface ValueMapping {
  on?: unknown
  off?: unknown
  true?: unknown
  false?: unknown
  [key: string]: unknown
}

// ============================================================================
// Time-Series Data Transformation Types
// ============================================================================

/**
 * Aggregation methods for time-series telemetry data.
 * - raw: Return all data points without aggregation
 * - latest: Return only the most recent value
 * - first: Return the oldest value in the time window
 * - avg: Average of all values
 * - min: Minimum value
 * - max: Maximum value
 * - sum: Sum of all values
 * - count: Count of data points
 * - delta: Change (last - first)
 * - rate: Rate of change per time unit
 */
export type TelemetryAggregate =
  | 'raw'        // 原始数据点
  | 'latest'     // 最新值
  | 'first'      // 第一个值
  | 'avg'        // 平均值
  | 'min'        // 最小值
  | 'max'        // 最大值
  | 'sum'        // 总和
  | 'count'      // 计数
  | 'delta'      // 变化量 (last - first)
  | 'rate'       // 变化率 per time unit

/**
 * Time window options for telemetry queries.
 */
export type TimeWindowType =
  | 'now'            // Current latest value (single point)
  | 'last_5min'      // Last 5 minutes
  | 'last_15min'     // Last 15 minutes
  | 'last_30min'     // Last 30 minutes
  | 'last_1hour'     // Last 1 hour
  | 'last_6hours'    // Last 6 hours
  | 'last_24hours'   // Last 24 hours
  | 'today'          // Today (from 00:00)
  | 'yesterday'      // Yesterday
  | 'this_week'      // This week
  | 'custom'         // Custom time range

/**
 * Time window configuration for telemetry queries.
 */
export interface TimeWindowConfig {
  type: TimeWindowType
  // When type='custom', specify start/end timestamps
  startTime?: number  // Unix timestamp in seconds
  endTime?: number    // Unix timestamp in seconds
}

/**
 * Chart view mode - how to interpret and display the data.
 * - timeseries: X-axis is time, show trends over time
 * - snapshot: Show current/aggregated values (comparison view)
 * - distribution: Show proportions (for pie/donut charts)
 * - histogram: Show frequency distribution
 */
export type ChartViewMode =
  | 'timeseries'     // 时序模式：X轴=时间
  | 'snapshot'       // 快照模式：显示当前值或聚合值对比
  | 'distribution'   // 分布模式：显示占比（适合饼图）
  | 'histogram'      // 直方图模式：显示频率分布

/**
 * How to handle missing/empty values in time series.
 */
export type FillMissingStrategy =
  | 'none'       // Leave as null/undefined
  | 'zero'       // Fill with 0
  | 'previous'   // Use previous value (forward fill)
  | 'linear'     // Linear interpolation between points

// ============================================================================
// Data Source Interface
// ============================================================================

export interface DataSource {
  type: DataSourceType
  endpoint?: string
  transform?: string
  refresh?: number
  params?: Record<string, unknown>
  staticValue?: unknown
  // Device-specific fields (for reading device telemetry)
  deviceId?: string
  property?: string
  // Metric-specific fields
  metricId?: string
  // Command-specific fields (for controlling devices)
  command?: string
  commandParams?: Record<string, unknown>
  valueMapping?: ValueMapping
  // Current value for command sources (for display)
  currentValue?: unknown

  // === Telemetry fields ===
  // Legacy: simple time range in hours (kept for backward compatibility)
  timeRange?: number
  // Legacy: max number of data points
  limit?: number
  // Legacy: simple aggregation
  aggregate?: 'raw' | 'avg' | 'min' | 'max' | 'sum'

  // === New: Time-series data transformation ===
  // Time window configuration
  timeWindow?: TimeWindowConfig
  // Extended aggregation method
  aggregateExt?: TelemetryAggregate
  // Chart view mode - how to interpret data
  chartViewMode?: ChartViewMode
  // Data sampling interval (seconds) - for downsampling
  sampleInterval?: number
  // How to handle missing values
  fillMissing?: FillMissingStrategy
  // Group dimension for multi-source data
  groupBy?: 'device' | 'metric' | 'time'

  // === Device-info fields ===
  infoProperty?: 'name' | 'status' | 'online' | 'last_seen' | 'device_type' | 'plugin_name' | 'adapter_id'
}

// Union type for single or multiple data sources
export type DataSourceOrList = DataSource | DataSource[]

// Check if a data source is a list
export function isDataSourceList(value: unknown): value is DataSource[] {
  return Array.isArray(value) && value.length > 0 && typeof value[0] === 'object' && 'type' in value[0]
}

// Normalize to array
export function normalizeDataSource(dataSource: DataSourceOrList | undefined): DataSource[] {
  if (!dataSource) return []
  return isDataSourceList(dataSource) ? dataSource : [dataSource]
}

// ============================================================================
// Component Type Definitions
// ============================================================================

/**
 * Generic Component Types
 * Basic reusable components for dashboards
 */
export type GenericComponentType =
  // Indicators
  | 'value-card'
  | 'led-indicator'
  | 'sparkline'
  | 'progress-bar'
  // Charts
  | 'line-chart'
  | 'area-chart'
  | 'bar-chart'
  | 'pie-chart'
  // Controls
  | 'toggle-switch'
  // Display & Content
  | 'image-display'
  | 'image-history'
  | 'web-display'
  | 'markdown-display'
  // Spatial & Media
  | 'map-display'
  | 'video-display'
  | 'custom-layer'

/**
 * Business Component Types
 * NeoTalk-specific business components
 */
export type BusinessComponentType =
  | 'agent-status-card'
  | 'decision-list'
  | 'device-control'
  | 'rule-status-grid'
  | 'transform-list'

/**
 * All Implemented Component Types
 *
 * Only includes components that are actually implemented.
 * Use this type instead of ComponentType for type safety.
 */
export type ImplementedComponentType = GenericComponentType | BusinessComponentType

/**
 * Component Type (Legacy)
 *
 * @deprecated Use ImplementedComponentType instead.
 * This type includes planned but unimplemented components.
 */
export type ComponentType = ImplementedComponentType

// ============================================================================
// Display Configuration Types
// ============================================================================

export type ColorScaleType = 'threshold' | 'gradient' | 'category'

export interface ColorScale {
  type: ColorScaleType
  stops: ColorStop[]
}

export interface ColorStop {
  value: number | string
  color: string
}

export interface Threshold {
  value: number
  operator: '>' | '<' | '=' | '>=' | '<='
  color: string
  icon?: string
}

export type Size = 'sm' | 'md' | 'lg'
export type Density = 'compact' | 'comfortable' | 'spacious'

export interface DisplayConfig {
  // Formatting
  format?: string
  unit?: string
  prefix?: string

  // Colors
  color?: string
  colorScale?: ColorScale

  // Ranges
  min?: number
  max?: number
  thresholds?: Threshold[]

  // Layout
  size?: Size
  density?: Density

  // Chart specific
  showLegend?: boolean
  showGrid?: boolean
  timeRange?: string
  aggregation?: string

  // Indicator specific
  showTrend?: boolean
  trendPeriod?: string
  showSparkline?: boolean
  icon?: string
}

// ============================================================================
// Component Position & Layout
// ============================================================================

export interface ComponentPosition {
  x: number
  y: number
  w: number
  h: number
  minW?: number
  minH?: number
  maxW?: number
  maxH?: number
}

/**
 * Default sizing constraints for dashboard components
 * Grid units are based on a 12-column grid system
 */
export interface ComponentSizeConstraints {
  minW: number
  minH: number
  defaultW: number
  defaultH: number
  maxW: number
  maxH: number
  preserveAspect?: boolean // Whether to maintain aspect ratio when resizing
}

/**
 * Default sizing constraints for dashboard components
 *
 * Grid units (based on rowHeight=60px):
 * - h:1 = 60px, h:2 = 120px, h:3 = 180px
 *
 * Mobile considerations (xs: 4 columns):
 * - minW should be <= 2 for most components to allow 2 columns per row
 * - minH should be <= 2 for mobile friendliness (120px max)
 */
export const COMPONENT_SIZE_CONSTRAINTS: Partial<Record<ImplementedComponentType, ComponentSizeConstraints>> = {
  // Indicators - compact for mobile
  'value-card': { minW: 2, minH: 1, defaultW: 3, defaultH: 2, maxW: 6, maxH: 4 },
  'led-indicator': { minW: 1, minH: 1, defaultW: 2, defaultH: 1, maxW: 3, maxH: 2, preserveAspect: true },
  'sparkline': { minW: 2, minH: 1, defaultW: 4, defaultH: 2, maxW: 8, maxH: 3 },
  'progress-bar': { minW: 2, minH: 1, defaultW: 4, defaultH: 1, maxW: 12, maxH: 3 },

  // Charts - slightly larger minimum for readability
  'line-chart': { minW: 3, minH: 2, defaultW: 6, defaultH: 4, maxW: 12, maxH: 8 },
  'area-chart': { minW: 3, minH: 2, defaultW: 6, defaultH: 4, maxW: 12, maxH: 8 },
  'bar-chart': { minW: 3, minH: 2, defaultW: 6, defaultH: 4, maxW: 12, maxH: 8 },
  'pie-chart': { minW: 2, minH: 2, defaultW: 4, defaultH: 4, maxW: 8, maxH: 8, preserveAspect: true },

  // Controls - very compact
  'toggle-switch': { minW: 1, minH: 1, defaultW: 2, defaultH: 1, maxW: 4, maxH: 2 },

  // Display & Content
  'image-display': { minW: 2, minH: 2, defaultW: 4, defaultH: 3, maxW: 12, maxH: 12 },
  'image-history': { minW: 3, minH: 3, defaultW: 6, defaultH: 4, maxW: 12, maxH: 12 },
  'web-display': { minW: 3, minH: 3, defaultW: 6, defaultH: 4, maxW: 12, maxH: 12 },
  'markdown-display': { minW: 2, minH: 2, defaultW: 4, defaultH: 3, maxW: 12, maxH: 12 },

  // Spatial & Media
  'map-display': { minW: 3, minH: 2, defaultW: 6, defaultH: 3, maxW: 12, maxH: 12 },
  'video-display': { minW: 3, minH: 2, defaultW: 6, defaultH: 4, maxW: 12, maxH: 12 },
  'custom-layer': { minW: 2, minH: 2, defaultW: 6, defaultH: 4, maxW: 12, maxH: 12 },

  // Business Components (deleted but kept for backward compatibility)
  'agent-status-card': { minW: 2, minH: 2, defaultW: 4, defaultH: 4, maxW: 6, maxH: 6, preserveAspect: true },
  'decision-list': { minW: 2, minH: 2, defaultW: 4, defaultH: 4, maxW: 6, maxH: 10 },
  'device-control': { minW: 2, minH: 2, defaultW: 4, defaultH: 3, maxW: 6, maxH: 5 },
  'rule-status-grid': { minW: 3, minH: 2, defaultW: 6, defaultH: 4, maxW: 12, maxH: 8 },
  'transform-list': { minW: 2, minH: 2, defaultW: 4, defaultH: 4, maxW: 6, maxH: 10 },
}

// ============================================================================
// Component Definitions
// ============================================================================

export interface BaseComponent {
  id: string
  type: ImplementedComponentType
  position: ComponentPosition
  title?: string
}

export interface GenericComponent extends BaseComponent {
  type: GenericComponentType
  dataSource?: DataSource
  display?: DisplayConfig
  actions?: ActionConfig[]
  config?: Record<string, unknown>
}

export interface BusinessComponent extends BaseComponent {
  type: BusinessComponentType
  config?: Record<string, unknown>
}

export type DashboardComponent = GenericComponent | BusinessComponent

// Type guards
export function isGenericComponent(component: DashboardComponent): component is GenericComponent {
  const genericTypes: GenericComponentType[] = [
    'value-card', 'led-indicator', 'sparkline', 'progress-bar',
    'line-chart', 'area-chart', 'bar-chart', 'pie-chart',
    'toggle-switch',
    'image-display', 'image-history', 'web-display', 'markdown-display',
    'map-display', 'video-display', 'custom-layer',
  ]
  return genericTypes.includes(component.type as GenericComponentType)
}

export function isBusinessComponent(component: DashboardComponent): component is BusinessComponent {
  return !isGenericComponent(component)
}

// ============================================================================
// Dashboard Types
// ============================================================================

export interface DashboardLayout {
  columns: number
  rows: 'auto' | number
  breakpoints: {
    lg: number
    md: number
    sm: number
    xs: number
  }
}

export interface Dashboard {
  id: string
  name: string
  layout: DashboardLayout
  components: DashboardComponent[]
  createdAt: number
  updatedAt: number
  isDefault?: boolean
}

export interface DashboardTemplate {
  id: string
  name: string
  description: string
  category: 'overview' | 'monitoring' | 'automation' | 'agents' | 'custom'
  icon?: string
  layout: DashboardLayout
  components: Omit<DashboardComponent, 'id'>[]
  requiredResources?: {
    devices?: number
    agents?: number
    rules?: number
  }
}

// ============================================================================
// Action Types
// ============================================================================

export type ActionType = 'api-call' | 'navigate' | 'dialog' | 'custom'

export interface ActionConfig {
  type: ActionType
  method?: string
  endpoint?: string
  path?: string
  dialog?: string
  confirm?: boolean
  handler?: string
}
