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
 * Value Card specific size configuration
 * Optimized for visual hierarchy rather than proportional scaling
 * - sm: Compact/Dense - for small cards and data-dense layouts
 * - md: Standard - balanced for most use cases
 * - lg: Prominent - for emphasis and larger containers
 */
export const valueCardSize = {
  sm: {
    // Minimal padding, tight spacing for compact display
    padding: 'p-2.5',
    headerPadding: 'pb-1.5',
    // Text: value is primary, keep it readable
    titleText: 'text-xs',
    labelText: 'text-[11px]',
    valueText: 'text-sm font-semibold',
    // Icons: smaller but still visible
    iconSize: 'w-3.5 h-3.5',
    iconContainer: 'w-7 h-7',
    // Gaps: tighter spacing
    contentGap: 'gap-2',
    itemGap: 'gap-1.5',
    // Border radius
    radius: 'rounded-md',
  },
  md: {
    // Balanced padding for standard display
    padding: 'p-3.5',
    headerPadding: 'pb-2',
    // Text: stronger value/label contrast for data focus
    titleText: 'text-xs uppercase tracking-wide',
    labelText: 'text-[11px]',
    valueText: 'text-lg font-semibold tracking-tight tabular-nums',
    // Icons: standard size
    iconSize: 'w-4 h-4',
    iconContainer: 'w-8 h-8',
    // Gaps: comfortable spacing
    contentGap: 'gap-2.5',
    itemGap: 'gap-2',
    // Border radius
    radius: 'rounded-lg',
  },
  lg: {
    // More padding for visual prominence
    padding: 'p-4.5',
    headerPadding: 'pb-2.5',
    // Text: emphasize value as the hero of the card
    titleText: 'text-xs uppercase tracking-wide',
    labelText: 'text-xs',
    valueText: 'text-2xl font-bold tracking-tight tabular-nums',
    // Icons: slightly larger for emphasis
    iconSize: 'w-5 h-5',
    iconContainer: 'w-10 h-10',
    // Gaps: more breathing room
    contentGap: 'gap-3',
    itemGap: 'gap-2',
    // Border radius
    radius: 'rounded-xl',
  },
} as const

export type ValueCardSize = keyof typeof valueCardSize

/**
 * Unified component size configuration
 * All components use consistent padding, gaps, and text scaling
 */
export const dashboardComponentSize = {
  xs: {
    // Padding (rem units) - smallest for 1x1 grid
    padding: 'p-2',
    headerPadding: 'pb-1.5',
    // Text sizes
    titleText: 'text-xs',
    labelText: 'text-[10px]',
    valueText: 'text-xs',
    // Icons
    iconSize: 'w-3 h-3',
    iconContainer: 'w-6 h-6',
    // Gaps
    contentGap: 'gap-1.5',
    itemGap: 'gap-1',
    // Border radius
    radius: 'rounded-md',
  },
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
/**
 * Base card styles for all dashboard components.
 *
 * Uses an opaque tonal surface (bg-card) rather than glass. Glass/backdrop-blur
 * is reserved for the navigation layer (TopNav, toolbars, drawers) per the
 * Apple HIG / Material 3 layered-surface model: content stays on a solid,
 * readable surface while only chrome floats above it. This also removes ~60
 * backdrop-filter layers from a dense dashboard, a significant GPU saving
 * (guidance: keep stacked backdrop-filter layers ≤ 2–3).
 */
export const dashboardCardBase = [
  // Layout
  'flex flex-col h-full w-full overflow-hidden',
  // Opaque tonal surface — readable content layer
  'bg-card',
  // Border, subtle shadow, and a 1px top highlight that conveys material
  // thickness (a quiet nod to physical surfacing, not flat-glass).
  'border border-glass-border shadow-sm',
  // Top edge highlight + hover lift, applied via the pseudo classes below.
  // Using ring on hover keeps it crisp without extra DOM.
  'rounded-lg',
  // Pseudo-element top highlight (set in index.css .dashboard-card-sheen)
  'dashboard-card-sheen',
].join(' ')

/**
 * Horizontal card styles (for value-card, compact cards, etc.)
 * Same visual styling as dashboardCardBase but with horizontal layout
 */
export const dashboardCardHorizontal = [
  // Layout - horizontal instead of vertical
  'flex flex-row h-full w-full overflow-hidden',
  // Opaque tonal surface
  'bg-card',
  // Border & subtle shadow
  'border border-glass-border shadow-sm',
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

// ============================================================================
// Shared Layout Tokens (dashboard + pages + dialogs)
// ============================================================================

/**
 * General card container — used for cards, panels, stat boxes across the app
 * Equivalent to `rounded-lg border bg-card shadow-sm` but centralized
 */
export const cardBase = [
  'rounded-lg border bg-card shadow-sm',
].join(' ')

/**
 * Interactive card — a clickable card surface (device/agent/extension cards,
 * list rows, etc.). Unifies the previously scattered hover treatments
 * (translate-y / shadow-md / border changes) into one consistent interaction.
 * Opaque surface: no backdrop-blur, keeping content legible and GPU-light.
 */
export const interactiveCard = [
  'bg-card rounded-lg border shadow-sm',
  'cursor-pointer transition-all duration-fast ease-spring-soft',
  'hover:shadow-md hover:-translate-y-0.5',
].join(' ')

/**
 * Interactive card hover effect — the lift/shadow transition only.
 *
 * Use this on cards that already carry their own surface styles (e.g. the
 * `<Card>` component which includes `cardBase`, or cards with a non-default
 * radius like `rounded-xl`) but should share the same hover interaction.
 * This avoids re-declaring `hover:shadow-md hover:-translate-y-0.5
 * transition-all duration-fast ease-spring-soft` at every call site.
 */
export const interactiveCardHover = [
  'transition-all duration-fast ease-spring-soft',
  'hover:shadow-md hover:-translate-y-0.5',
].join(' ')

/**
 * Card with padding — most common card pattern (card + standard padding)
 */
export const cardPadded = [
  'rounded-lg border bg-card shadow-sm p-4',
].join(' ')

/**
 * Dialog/Panel section header — sticky top bar with title
 * Used in: dialog headers, sidebar headers, section headers
 */
export const sectionHeader = [
  'flex items-center justify-between',
  'px-4 py-3 border-b shrink-0',
].join(' ')

/**
 * Dialog header — larger padding variant for full-screen dialogs.
 *
 * Uses bg-popover (not bg-background) so the header matches the dialog body:
 * in light mode --background is oklch(0.985) (slightly off-white) while
 * --popover is pure white, so a bg-background header reads as a dirty gray
 * strip on top of a pure-white dialog body. bg-popover is opaque in both
 * themes and matches the surrounding dialog Content (which is also bg-popover).
 */
export const dialogHeader = [
  'flex items-center justify-between',
  'px-4 py-4 border-b shrink-0 bg-popover',
].join(' ')

/**
 * Subsection header — smaller, used for grouping within a section
 */
export const subSectionHeader = [
  'flex items-center gap-2',
  'px-3 py-2 border-b shrink-0',
  'text-xs font-medium text-muted-foreground bg-muted-30',
].join(' ')

/**
 * Scrollable body — fills remaining space with scroll
 */
export const scrollableBody = [
  'flex-1 min-h-0 overflow-y-auto overflow-x-hidden',
].join(' ')

/**
 * Standard transition — used for interactive elements
 */
export const transitionDefault = 'transition-all duration-200'
