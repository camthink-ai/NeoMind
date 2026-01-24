/**
 * Design Tokens - Size (Responsive)
 *
 * Components fill their containers completely.
 * Size props only control relative proportions, not fixed dimensions.
 */

// Spacing scale (base unit: 4px)
export const spacing = {
  xs: '0.25rem',   // 4px
  sm: '0.5rem',    // 8px
  md: '1rem',      // 16px
  lg: '1.5rem',    // 24px
  xl: '2rem',      // 32px
  '2xl': '3rem',   // 48px
  '3xl': '4rem',   // 64px
} as const

export type SpacingToken = keyof typeof spacing

// Component size presets - controls relative scale, NOT dimensions
// All components are 100% width/height of their container
export const componentSize = {
  sm: {
    // Scale factor for child elements (0.85x base size)
    scale: '0.85',
    // Responsive padding as percentage of container
    padding: 'p-[3%]',
    // Text scale multiplier
    textScale: 'text-[0.85em]',
    // Gap scale
    gap: 'gap-[3%]',
  },
  md: {
    scale: '1',
    padding: 'p-[4%]',
    textScale: 'text-[1em]',
    gap: 'gap-[4%]',
  },
  lg: {
    scale: '1.15',
    padding: 'p-[5%]',
    textScale: 'text-[1.15em]',
    gap: 'gap-[5%]',
  },
} as const

export type ComponentSize = keyof typeof componentSize

// Chart-specific sizes
export const chartSize = {
  sm: { strokeWidth: 10, valueText: 'text-[0.9em]', labelText: 'text-[0.75em]' },
  md: { strokeWidth: 12, valueText: 'text-[1em]', labelText: 'text-[0.8em]' },
  lg: { strokeWidth: 14, valueText: 'text-[1.1em]', labelText: 'text-[0.85em]' },
  xl: { strokeWidth: 16, valueText: 'text-[1.2em]', labelText: 'text-[0.9em]' },
} as const

export type ChartSize = keyof typeof chartSize

// Grid breakpoints
export const gridBreakpoints = {
  xs: 480,
  sm: 768,
  md: 996,
  lg: 1200,
  xl: 1600,
} as const

export type GridBreakpoint = keyof typeof gridBreakpoints

// Grid column widths (out of 12)
export const gridColumns = 12

// Component default sizes (in grid units)
// Updated with minW: 1 for mobile support
export const defaultComponentSizes = {
  // Indicators
  'value-card': { w: 2, h: 2, minW: 1, minH: 1 },
  'led-indicator': { w: 2, h: 2, minW: 1, minH: 1 },
  'sparkline': { w: 4, h: 2, minW: 2, minH: 1 },
  'progress-bar': { w: 4, h: 2, minW: 2, minH: 1 },

  // Charts
  'line-chart': { w: 6, h: 4, minW: 2, minH: 2 },
  'area-chart': { w: 6, h: 4, minW: 2, minH: 2 },
  'bar-chart': { w: 6, h: 4, minW: 2, minH: 2 },
  'pie-chart': { w: 4, h: 4, minW: 2, minH: 2 },
  'donut-chart': { w: 4, h: 4, minW: 2, minH: 2 },
  'gauge-chart': { w: 4, h: 3, minW: 2, minH: 2 },

  // Controls
  'toggle-switch': { w: 2, h: 1, minW: 1, minH: 1 },
  'button-group': { w: 3, h: 1, minW: 1, minH: 1 },
  'dropdown': { w: 3, h: 1, minW: 1, minH: 1 },
  'input-field': { w: 3, h: 1, minW: 1, minH: 1 },

  // Tables & Lists
  'data-table': { w: 6, h: 5, minW: 2, minH: 2 },
  'status-list': { w: 4, h: 5, minW: 2, minH: 2 },
  'log-feed': { w: 4, h: 5, minW: 2, minH: 2 },

  // Layout & Content
  'tabs': { w: 6, h: 4, minW: 2, minH: 2 },
  'heading': { w: 4, h: 1, minW: 1, minH: 1 },
  'alert-banner': { w: 4, h: 1, minW: 1, minH: 1 },

  // Business Components
  'agent-status-card': { w: 4, h: 4, minW: 2, minH: 2 },
  'decision-list': { w: 4, h: 5, minW: 2, minH: 2 },
  'device-control': { w: 4, h: 3, minW: 2, minH: 2 },
  'rule-status-grid': { w: 6, h: 4, minW: 2, minH: 2 },
  'transform-list': { w: 4, h: 5, minW: 2, minH: 2 },
} as const

// Responsive column configuration
export const responsiveCols = {
  lg: 12,
  md: 10,
  sm: 6,
  xs: 4,  // Mobile: 4 columns, so minW: 1 = 1 column
} as const

// ============================================================================
// Dashboard Component Unified Styles
// ============================================================================

/**
 * Unified component size configuration
 * All components use consistent padding, gaps, and text scaling
 */
export const dashboardComponentSize = {
  sm: {
    // Padding (rem units)
    padding: 'p-3',
    headerPadding: 'pb-2',
    // Text sizes
    titleText: 'text-sm',
    labelText: 'text-xs',
    valueText: 'text-sm',
    // Icons
    iconSize: 'w-4 h-4',
    iconContainer: 'w-8 h-8',
    // Gaps
    contentGap: 'gap-2',
    itemGap: 'gap-1.5',
    // Border radius
    radius: 'rounded-lg',
  },
  md: {
    padding: 'p-4',
    headerPadding: 'pb-3',
    titleText: 'text-sm',
    labelText: 'text-xs',
    valueText: 'text-base',
    iconSize: 'w-4 h-4',
    iconContainer: 'w-9 h-9',
    contentGap: 'gap-3',
    itemGap: 'gap-2',
    radius: 'rounded-lg',
  },
  lg: {
    padding: 'p-5',
    headerPadding: 'pb-4',
    titleText: 'text-base',
    labelText: 'text-sm',
    valueText: 'text-lg',
    iconSize: 'w-5 h-5',
    iconContainer: 'w-10 h-10',
    contentGap: 'gap-4',
    itemGap: 'gap-2.5',
    radius: 'rounded-xl',
  },
} as const

export type DashboardComponentSize = keyof typeof dashboardComponentSize

/**
 * Base card styles for all dashboard components
 * Use these classes for consistent card appearance
 */
export const dashboardCardBase = [
  // Layout
  'flex flex-col h-full w-full overflow-hidden',
  // Background & border
  'bg-card/50 backdrop-blur',
  // Border & shadow
  'border border-border/50 shadow-sm hover:shadow-md transition-shadow',
  // Radius
  'rounded-lg',
].join(' ')

/**
 * Card content wrapper styles
 * Ensures content fills available space correctly
 */
export const dashboardCardContent = [
  // Layout
  'flex flex-col flex-1 min-h-0 min-w-0',
  // Overflow
  'overflow-hidden',
].join(' ')

/**
 * Card header styles
 */
export const dashboardCardHeader = [
  // Layout
  'flex flex-row items-start justify-between gap-2',
  // Flex shrink to prevent header from expanding
  'flex-shrink-0',
].join(' ')

/**
 * Scrollable content area for lists/tables
 */
export const dashboardScrollableContent = [
  // Layout
  'flex-1 overflow-y-auto overflow-x-hidden',
  // Scrollbar styling
  'scrollbar-thin scrollbar-thumb-muted-foreground/20 scrollbar-track-transparent',
].join(' ')
