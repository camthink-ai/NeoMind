/**
 * Dashboard type definitions for NeoMind
 *
 * Two-layer component system:
 * - Generic Components: Reusable IoT/dashboard components
 * - Business Components: NeoMind-specific components
 */

// ============================================================================
// Data Source Types
// ============================================================================

/**
 * Legacy DataSource type — mixes source and mode into a single discriminator.
 * @deprecated Use `DataSourceSource` + `DataSourceMode` instead.
 */
export type DataSourceType = 'device' | 'metric' | 'command' | 'telemetry' | 'device-info' | 'system' | 'extension' | 'extension-metric' | 'extension-command' | 'transform' | 'agent'

/** Unified source type — identifies where data comes from */
export type DataSourceSource = 'device' | 'extension' | 'system' | 'transform' | 'ai' | 'expression'
/** Unified mode — identifies how data is consumed */
export type DataSourceMode = 'latest' | 'timeseries' | 'command' | 'info' | 'list'

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

// ============================================================================
// Data Source — shared base fields
// ============================================================================

interface DataSourceBase {
  endpoint?: string
  transform?: string
  refresh?: number
  params?: Record<string, unknown>
  staticValue?: unknown
  // CustomLayer specific
  text?: string
  icon?: string
  // Legacy telemetry fields
  timeRange?: number
  limit?: number
  aggregate?: 'raw' | 'avg' | 'min' | 'max' | 'sum'
  timeWindow?: TimeWindowConfig
  aggregateExt?: TelemetryAggregate
}

// ============================================================================
// Data Source — discriminated union by type
// ============================================================================

// Device-related sources (WS event-driven)
export interface DeviceDataSource extends DataSourceBase {
  type: 'device'
  sourceId: string
  property?: string
}

export interface MetricDataSource extends DataSourceBase {
  type: 'metric'
  sourceId: string
  metricId?: string
}

export interface CommandDataSource extends DataSourceBase {
  type: 'command'
  sourceId: string
  command?: string
  commandParams?: Record<string, unknown>
  valueMapping?: ValueMapping
  currentValue?: unknown
}

export interface TelemetryDataSource extends DataSourceBase {
  type: 'telemetry'
  sourceId: string
  metricId?: string
}

export interface DeviceInfoDataSource extends DataSourceBase {
  type: 'device-info'
  sourceId: string
  infoProperty?: 'name' | 'status' | 'online' | 'last_seen' | 'device_type' | 'plugin_name' | 'adapter_id'
}

// System (polled)
export interface SystemDataSource extends DataSourceBase {
  type: 'system'
  systemMetric?: 'uptime' | 'cpu_count' | 'total_memory' | 'used_memory' | 'free_memory' | 'available_memory' | 'memory_percent' | 'platform' | 'arch' | 'version'
}

// Extension (WS event-driven)
export interface ExtensionDataSource extends DataSourceBase {
  type: 'extension'
  extensionId?: string
  extensionMetric?: string
  extensionCommand?: string
  extensionDisplayName?: string
  extensionDataType?: string
  extensionUnit?: string
  sourceId?: string
}

export interface ExtensionMetricDataSource extends DataSourceBase {
  type: 'extension-metric'
  extensionId?: string
  extensionMetric?: string
  sourceId?: string
}

export interface ExtensionCommandDataSource extends DataSourceBase {
  type: 'extension-command'
  extensionId?: string
  extensionCommand?: string
  sourceId?: string
}

// Transform (polled via telemetry)
export interface TransformDataSource extends DataSourceBase {
  type: 'transform'
  transformId?: string
  sourceId?: string
  metricId?: string
}

// Agent
export interface AgentDataSource extends DataSourceBase {
  type: 'agent'
  agentId?: string
  sourceId?: string
}

// ============================================================================
// Data Source — union + legacy interface
// ============================================================================

/**
 * Legacy flat DataSource interface.
 *
 * This interface is kept for backward compatibility with existing consumers.
 */
export interface DataSource {
  type: DataSourceType
  endpoint?: string
  transform?: string
  refresh?: number
  params?: Record<string, unknown>
  staticValue?: unknown
  sourceId?: string
  property?: string
  metricId?: string
  command?: string
  commandParams?: Record<string, unknown>
  valueMapping?: ValueMapping
  currentValue?: unknown
  text?: string
  icon?: string
  timeRange?: number
  limit?: number
  aggregate?: 'raw' | 'avg' | 'min' | 'max' | 'sum'
  timeWindow?: TimeWindowConfig
  aggregateExt?: TelemetryAggregate
  infoProperty?: 'name' | 'status' | 'online' | 'last_seen' | 'device_type' | 'plugin_name' | 'adapter_id'
  systemMetric?: 'uptime' | 'cpu_count' | 'total_memory' | 'used_memory' | 'free_memory' | 'available_memory' | 'memory_percent' | 'platform' | 'arch' | 'version'
  extensionId?: string
  extensionMetric?: string
  extensionCommand?: string
  extensionDisplayName?: string
  extensionDataType?: string
  extensionUnit?: string
  transformId?: string
  agentId?: string
  /** Inline computed value (source:'expression'). Refs `device:<id>:<field>`
   *  or `<id>.<field>`; arithmetic + avg/sum/min/max/abs/round/floor/ceil/sqrt/pow.
   *  No pre-created transform needed. */
  expr?: string
  // Unified fields (Phase 1)
  source?: DataSourceSource
  id?: string
  field?: string
  mode?: DataSourceMode
}

// Union type for single or multiple data sources
export type DataSourceOrList = DataSource | DataSource[]

// Check if a data source is a list
export function isDataSourceList(value: unknown): value is DataSource[] {
  return Array.isArray(value) && value.length > 0 && typeof value[0] === 'object' && 'type' in value[0]
}

/**
 * Convert hours to the closest TimeWindowType (reverse of timeWindowToHours).
 * Used to resolve legacy `timeRange` (hours) to canonical `timeWindow`.
 */
export function hoursToTimeWindow(hours: number): TimeWindowConfig {
  if (hours === 0) return { type: 'now' }
  const mapping: [number, TimeWindowType][] = [
    [5 / 60, 'last_5min'],
    [15 / 60, 'last_15min'],
    [30 / 60, 'last_30min'],
    [1, 'last_1hour'],
    [6, 'last_6hours'],
    [24, 'last_24hours'],
    [24 * 7, 'this_week'],
  ]
  for (const [h, type] of mapping) {
    if (hours <= h) return { type }
  }
  return { type: 'last_24hours' }
}

/**
 * Normalize legacy fields to canonical fields on a single DataSource.
 * Called once at entry point so all downstream consumers can read directly.
 *
 * - aggregate / aggregateExt → aggregateExt
 * - timeRange / timeWindow → timeWindow
 */
export function resolveDataSource(ds: DataSource): DataSource {
  // Resolve aggregate: aggregateExt is canonical, fallback to aggregate
  const aggregateExt = ds.aggregateExt ?? (ds.aggregate as TelemetryAggregate | undefined) ?? undefined

  // Resolve timeWindow: prefer explicit, fallback from legacy timeRange
  const timeWindow = ds.timeWindow ?? (ds.timeRange != null ? hoursToTimeWindow(ds.timeRange) : undefined)

  return migrateToUnified({
    ...ds,
    ...(aggregateExt !== undefined && { aggregateExt }),
    ...(timeWindow !== undefined && { timeWindow }),
  })
}

// Normalize to array (resolves legacy fields)
export function normalizeDataSource(dataSource: DataSourceOrList | undefined): DataSource[] {
  if (!dataSource) return []
  if (Array.isArray(dataSource) && dataSource.length === 0) return []
  const arr = isDataSourceList(dataSource) ? dataSource : [dataSource]
  return arr.filter(Boolean).map(resolveDataSource)
}

/** Get the source identifier from a DataSource */
export function getSourceId(ds: DataSource): string | undefined {
  return ds.sourceId
}

// ============================================================================
// Unified field migration + helpers (Phase 1)
// ============================================================================

/**
 * Migrate a legacy DataSource to carry unified source/mode/id/field fields.
 * Idempotent — already-migrated sources pass through unchanged.
 */
export function migrateToUnified(ds: DataSource): DataSource {
  if (ds.source && ds.mode && ds.id !== undefined && ds.field !== undefined) return ds
  const m = { ...ds }
  switch (ds.type) {
    case 'device':
      m.source = 'device'; m.id = ds.sourceId; m.field = ds.property || 'value'; m.mode = 'latest'; break
    case 'metric':
      m.source = 'device'; m.id = ds.sourceId; m.field = ds.metricId || 'value'; m.mode = 'latest'; break
    case 'command':
      m.source = 'device'; m.id = ds.sourceId; m.field = ds.command; m.mode = 'command'; break
    case 'device-info':
      m.source = 'device'; m.id = ds.sourceId; m.field = ds.infoProperty; m.mode = 'info'; break
    case 'telemetry':
      m.source = 'device'; m.id = ds.sourceId; m.field = ds.metricId || 'value'; m.mode = 'timeseries'; break
    case 'extension':
      m.source = 'extension'; m.id = ds.extensionId || ds.sourceId; m.field = ds.extensionMetric
      m.mode = (ds.timeRange || ds.timeWindow) ? 'timeseries' : 'latest'; break
    case 'extension-metric':
      m.source = 'extension'; m.id = ds.extensionId || ds.sourceId; m.field = ds.extensionMetric; m.mode = 'timeseries'; break
    case 'extension-command':
      m.source = 'extension'; m.id = ds.extensionId || ds.sourceId; m.field = ds.extensionCommand; m.mode = 'command'; break
    case 'system':
      m.source = 'system'; m.id = 'neomind'; m.field = ds.systemMetric; m.mode = 'latest'; break
    case 'transform':
      m.source = 'transform'; m.id = ds.transformId || ds.sourceId?.replace('transform:', ''); m.field = ds.metricId; m.mode = 'timeseries'; break
    case 'agent':
      m.source = 'ai'; m.id = ds.agentId || ds.sourceId; m.field = 'status'; m.mode = 'latest'; break
  }
  // Reverse-populate legacy fields from unified fields so that code
  // reading only legacy fields (ds.sourceId, ds.metricId, etc.) works
  // correctly for DataSources created with unified-only fields.
  if (m.id && !m.sourceId) m.sourceId = m.id
  if (m.field) {
    if (!m.metricId && !m.property && !m.systemMetric && !m.command && !m.extensionMetric && !m.extensionCommand && !m.infoProperty) {
      // Infer which legacy field to set based on source/mode
      if (m.source === 'device') {
        if (m.mode === 'command') m.command = m.field
        else if (m.mode === 'info') (m as Record<string, unknown>).infoProperty = m.field
        else if (m.mode === 'latest' && m.type !== 'telemetry') m.property = m.field
        else m.metricId = m.field
      } else if (m.source === 'extension') {
        if (m.mode === 'command') m.extensionCommand = m.field
        else m.extensionMetric = m.field
      } else if (m.source === 'system') {
        m.systemMetric = m.field as any
      } else {
        m.metricId = m.field
      }
    }
    if (m.source === 'extension' && !m.extensionId) m.extensionId = m.id
  }
  return m
}

/** Unified field accessors — prefer these over legacy fields for new code. */
export function getUnifiedSource(ds: DataSource): DataSourceSource | undefined { return ds.source }
export function getUnifiedMode(ds: DataSource): DataSourceMode | undefined { return ds.mode }
export function getUnifiedId(ds: DataSource): string | undefined { return ds.id ?? ds.sourceId }
export function getUnifiedField(ds: DataSource): string | undefined {
  return ds.field ?? ds.metricId ?? ds.property ?? ds.systemMetric ?? ds.command ?? ds.extensionMetric ?? ds.extensionCommand ?? ds.infoProperty
}

/**
 * Get the device_id that WS events use for this data source.
 *
 * Transform/AI sources get prefixed ("transform:{id}" / "ai:{id}") to match
 * the backend's event publishing namespace (storage_device_id format).
 * Device/extension sources use the bare unified ID.
 *
 * Use this for WS event matching (telemetryByDevice keys, relevantDeviceIds,
 * processTelemetryEvent device_id comparison). For API fetches, the prefix
 * is added separately in useTelemetrySource.
 */
export function getEventDeviceId(ds: DataSource): string | undefined {
  const id = getUnifiedId(ds)
  if (!id) return undefined
  const source = getUnifiedSource(ds)
  if (source === 'transform' && !id.startsWith('transform:')) {
    return `transform:${id}`
  }
  if (source === 'ai' && !id.startsWith('ai:')) {
    return `ai:${id}`
  }
  return id
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
 * NeoMind-specific business components
 */
export type BusinessComponentType =
  | 'agent-monitor-widget'
  | 'ai-analyst'

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
  'value-card': { minW: 2, minH: 1, defaultW: 2, defaultH: 1, maxW: 4, maxH: 2 },
  'led-indicator': { minW: 1, minH: 1, defaultW: 2, defaultH: 1, maxW: 3, maxH: 2, preserveAspect: true },
  'sparkline': { minW: 2, minH: 1, defaultW: 4, defaultH: 2, maxW: 8, maxH: 3 },
  'progress-bar': { minW: 2, minH: 1, defaultW: 4, defaultH: 1, maxW: 12, maxH: 3 },

  // Charts - need enough space for axes and legend
  'line-chart': { minW: 4, minH: 3, defaultW: 6, defaultH: 4, maxW: 12, maxH: 8 },
  'area-chart': { minW: 4, minH: 3, defaultW: 6, defaultH: 4, maxW: 12, maxH: 8 },
  'bar-chart': { minW: 4, minH: 3, defaultW: 6, defaultH: 4, maxW: 12, maxH: 8 },
  'pie-chart': { minW: 3, minH: 3, defaultW: 4, defaultH: 4, maxW: 8, maxH: 8, preserveAspect: true },

  // Controls - very compact
  'toggle-switch': { minW: 1, minH: 1, defaultW: 2, defaultH: 1, maxW: 4, maxH: 2 },

  // Display & Content
  'image-display': { minW: 2, minH: 2, defaultW: 4, defaultH: 3, maxW: 12, maxH: 12 },
  'image-history': { minW: 4, minH: 3, defaultW: 6, defaultH: 5, maxW: 12, maxH: 12 },
  'web-display': { minW: 3, minH: 3, defaultW: 6, defaultH: 4, maxW: 12, maxH: 12 },
  'markdown-display': { minW: 2, minH: 2, defaultW: 4, defaultH: 3, maxW: 12, maxH: 12 },

  // Spatial & Media
  'map-display': { minW: 4, minH: 3, defaultW: 6, defaultH: 4, maxW: 12, maxH: 12 },
  'video-display': { minW: 3, minH: 2, defaultW: 6, defaultH: 4, maxW: 12, maxH: 12 },
  'custom-layer': { minW: 2, minH: 2, defaultW: 6, defaultH: 4, maxW: 12, maxH: 12 },

  // Business Components
  'agent-monitor-widget': { minW: 4, minH: 4, defaultW: 6, defaultH: 5, maxW: 12, maxH: 8 },
  'ai-analyst': { minW: 3, minH: 3, defaultW: 4, defaultH: 5, maxW: 8, maxH: 8 },
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
  dataSource?: DataSourceOrList
  display?: DisplayConfig
  actions?: ActionConfig[]
  config?: Record<string, unknown>
}

export interface BusinessComponent extends BaseComponent {
  type: BusinessComponentType
  dataSource?: DataSourceOrList
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
  /** Manual sort order (lower = higher in the sidebar). Assigned server-side. */
  sortOrder?: number
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
