/**
 * Design System - Base Component Types
 *
 * Unified base interfaces for all dashboard components.
 */

import type { DataSource } from '@/types/dashboard'
import type { DashboardComponentSize } from '../tokens/size'

// ============================================================================
// Base Component Props
// ============================================================================

export interface BaseComponentProps {
  // Identification
  id?: string

  // Content
  title?: string
  description?: string

  // Size
  size?: DashboardComponentSize

  // Data binding
  dataSource?: DataSource

  // Styling
  className?: string
  showCard?: boolean

  // States
  loading?: boolean
  error?: string
  empty?: boolean

  // Accessibility
  ariaLabel?: string
}

// ============================================================================
// Display Props (for showing values)
// ============================================================================

export interface DisplayProps {
  // Formatting
  format?: string
  unit?: string
  prefix?: string
  suffix?: string

  // Precision
  precision?: number
  minDecimalPlaces?: number
  maxDecimalPlaces?: number

  // Locale
  locale?: string
}

// ============================================================================
// Color Props (for conditional coloring)
// ============================================================================

export interface ColorProps {
  // Static color
  color?: string

  // Color based on value thresholds
  thresholds?: Array<{
    value: number
    operator: '>' | '<' | '=' | '>=' | '<='
    color: string
  }>

  // Color scale
  colorScale?: 'success' | 'warning' | 'error' | 'info' | 'neutral'

  // Invert colors (for dark mode)
  invertColor?: boolean
}

// ============================================================================
// Trend Props (for showing trends)
// ============================================================================

export interface TrendProps {
  showTrend?: boolean
  trendValue?: number
  trendPeriod?: string
  trendLabel?: string
}

// ============================================================================
// Card Props (for card wrapping)
// ============================================================================

export interface CardProps {
  showCard?: boolean
  showHeader?: boolean
  showFooter?: boolean
  cardClassName?: string
  headerClassName?: string
  contentClassName?: string
  footerClassName?: string
  clickable?: boolean
  onClick?: () => void
}

// ============================================================================
// Chart Props (common chart options)
// ============================================================================

export interface ChartProps {
  // Data
  data?: unknown[]

  // Dimensions
  height?: number | 'auto'
  width?: number | 'auto'
  aspectRatio?: number

  // Display options
  showLegend?: boolean
  showTooltip?: boolean
  showGrid?: boolean
  showLabels?: boolean

  // Colors
  colors?: string[]

  // Animation
  animate?: boolean
  animationDuration?: number
}

// ============================================================================
// Control Props (for interactive components)
// ============================================================================`

export interface ControlProps {
  // State
  value?: unknown
  defaultValue?: unknown
  disabled?: boolean
  readonly?: boolean

  // Callbacks
  onChange?: (value: unknown) => void
  onBeforeChange?: (value: unknown) => boolean
  onAfterChange?: (value: unknown) => void

  // Validation
  min?: number
  max?: number
  step?: number
  required?: boolean

  // Loading state for async changes
  sending?: boolean
}

// ============================================================================
// Layout Props (for positioning)
// ============================================================================

export interface LayoutProps {
  // Grid position
  x?: number
  y?: number
  w?: number  // width in grid columns
  h?: number  // height in grid rows

  // Constraints
  minW?: number
  minH?: number
  maxW?: number
  maxH?: number

  // Responsive behavior
  static?: boolean  // don't allow drag/resize
  dragHandle?: string  // CSS selector for drag handle
}

// ============================================================================
// Combined Props Types
// ============================================================================

export interface ValueCardProps extends BaseComponentProps, DisplayProps, ColorProps, TrendProps, CardProps {
  variant?: 'default' | 'vertical' | 'compact'
}

export interface ControlCardProps extends BaseComponentProps, ControlProps, CardProps {
  variant?: 'default' | 'icon' | 'slider' | 'pill'
  trueLabel?: string
  falseLabel?: string
}

export interface ChartCardProps extends BaseComponentProps, ChartProps, CardProps {
  title?: string
}

// ============================================================================
// Component States
// ============================================================================

export type ComponentState = 'idle' | 'loading' | 'success' | 'error' | 'empty'

export interface ComponentStateProps {
  state: ComponentState
  error?: string
  emptyMessage?: string
  onRetry?: () => void
}
