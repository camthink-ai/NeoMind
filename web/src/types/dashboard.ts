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

export type DataSourceType = 'api' | 'websocket' | 'static' | 'computed' | 'device' | 'metric' | 'command' | 'telemetry'

export interface ValueMapping {
  on?: unknown
  off?: unknown
  true?: unknown
  false?: unknown
  [key: string]: unknown
}

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
  // Telemetry-specific fields (for historical time-series data)
  timeRange?: number // Hours of history to fetch (default: 1)
  limit?: number // Max number of data points (default: 50)
  aggregate?: 'raw' | 'avg' | 'min' | 'max' | 'sum' // Aggregation method (default: raw)
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
  | 'donut-chart'
  | 'gauge-chart'
  // Controls
  | 'toggle-switch'
  | 'button-group'
  | 'dropdown'
  | 'input-field'
  // Lists & Tables
  | 'data-table'
  | 'status-list'
  | 'log-feed'
  // Layout & Content
  | 'tabs'
  | 'heading'
  | 'alert-banner'

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
  'donut-chart': { minW: 2, minH: 2, defaultW: 4, defaultH: 4, maxW: 8, maxH: 8, preserveAspect: true },
  'gauge-chart': { minW: 2, minH: 1, defaultW: 4, defaultH: 3, maxW: 8, maxH: 6 },

  // Controls - very compact
  'toggle-switch': { minW: 1, minH: 1, defaultW: 2, defaultH: 1, maxW: 4, maxH: 2 },
  'button-group': { minW: 2, minH: 1, defaultW: 3, defaultH: 1, maxW: 6, maxH: 2 },
  'dropdown': { minW: 2, minH: 1, defaultW: 3, defaultH: 1, maxW: 4, maxH: 2 },
  'input-field': { minW: 2, minH: 1, defaultW: 3, defaultH: 1, maxW: 6, maxH: 2 },

  // Tables & Lists - need more height
  'data-table': { minW: 3, minH: 2, defaultW: 6, defaultH: 5, maxW: 12, maxH: 12 },
  'status-list': { minW: 2, minH: 2, defaultW: 4, defaultH: 4, maxW: 6, maxH: 10 },
  'log-feed': { minW: 2, minH: 2, defaultW: 4, defaultH: 4, maxW: 8, maxH: 12 },

  // Layout & Content
  'tabs': { minW: 2, minH: 2, defaultW: 6, defaultH: 4, maxW: 12, maxH: 8 },
  'heading': { minW: 1, minH: 1, defaultW: 4, defaultH: 1, maxW: 12, maxH: 2 },
  'alert-banner': { minW: 2, minH: 1, defaultW: 4, defaultH: 1, maxW: 12, maxH: 2 },

  // Business Components
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
    'line-chart', 'area-chart', 'bar-chart', 'pie-chart', 'donut-chart', 'gauge-chart',
    'toggle-switch', 'button-group', 'dropdown', 'input-field',
    'data-table', 'status-list', 'log-feed',
    'tabs', 'heading', 'alert-banner',
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
