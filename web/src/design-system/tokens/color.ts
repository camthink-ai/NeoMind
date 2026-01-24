/**
 * Design Tokens - Color
 *
 * Unified color system for all dashboard components.
 * Uses OKLCH for perceptual uniformity.
 */

// Chart colors (using OKLCH for better perceptual uniformity)
export const chartColors = {
  1: 'oklch(0.646 0.222 264.38)',   // Blue/Purple
  2: 'oklch(0.646 0.222 142.5)',    // Green
  3: 'oklch(0.646 0.222 48.85)',    // Yellow
  4: 'oklch(0.646 0.222 24.85)',    // Orange
  5: 'oklch(0.646 0.222 304.38)',   // Pink
  6: 'oklch(0.646 0.222 188.38)',   // Cyan
} as const

export type ChartColor = keyof typeof chartColors

// Status colors
export const statusColors = {
  success: 'oklch(0.646 0.222 142.5)',   // Green
  warning: 'oklch(0.646 0.222 85.85)',    // Yellow/Orange
  error: 'oklch(0.576 0.222 25.85)',      // Red (darker for text)
  info: 'oklch(0.646 0.222 264.38)',      // Blue
  neutral: 'oklch(0.551 0.0 264.38)',     // Gray
} as const

export type StatusColor = keyof typeof statusColors

// Status colors with opacity for backgrounds
export const statusBgColors = {
  success: 'oklch(0.646 0.222 142.5 / 0.15)',
  warning: 'oklch(0.646 0.222 85.85 / 0.15)',
  error: 'oklch(0.576 0.222 25.85 / 0.15)',
  info: 'oklch(0.646 0.222 264.38 / 0.15)',
  neutral: 'oklch(0.551 0.0 264.38 / 0.1)',
} as const

// Semantic color mappings
export const semanticColors = {
  // Device states
  online: statusColors.success,
  offline: statusColors.neutral,
  error: statusColors.error,
  unknown: statusColors.neutral,

  // Agent states
  idle: statusColors.neutral,
  running: statusColors.info,
  paused: statusColors.warning,
  completed: statusColors.success,
  failed: statusColors.error,

  // Trend directions
  up: statusColors.success,    // Green for positive
  down: statusColors.error,    // Red for negative
  neutral: statusColors.neutral,
} as const

export type SemanticColor = keyof typeof semanticColors

// CSS custom properties (for global styles)
export const cssVars = {
  // Chart colors
  '--color-chart-1': chartColors[1],
  '--color-chart-2': chartColors[2],
  '--color-chart-3': chartColors[3],
  '--color-chart-4': chartColors[4],
  '--color-chart-5': chartColors[5],
  '--color-chart-6': chartColors[6],

  // Status colors
  '--color-status-success': statusColors.success,
  '--color-status-warning': statusColors.warning,
  '--color-status-error': statusColors.error,
  '--color-status-info': statusColors.info,
  '--color-status-neutral': statusColors.neutral,
} as const

// Helper to get chart color by index
export function getChartColor(index: number): string {
  return chartColors[(index % Object.keys(chartColors).length + 1) as ChartColor]
}

// Helper to get status color with opacity
export function getStatusColor(
  status: keyof typeof statusColors,
  opacity: number = 1
): string {
  const color = statusColors[status]
  if (opacity < 1) {
    return color.replace(')', ` / ${opacity})`)
  }
  return color
}

// Color scale classes for Tailwind
export const colorScaleClasses = {
  green: {
    text: 'text-green-600 dark:text-green-400',
    bg: 'bg-green-500/15',
    border: 'border-green-500/20',
  },
  yellow: {
    text: 'text-yellow-600 dark:text-yellow-400',
    bg: 'bg-yellow-500/15',
    border: 'border-yellow-500/20',
  },
  red: {
    text: 'text-red-600 dark:text-red-400',
    bg: 'bg-red-500/15',
    border: 'border-red-500/20',
  },
  blue: {
    text: 'text-blue-600 dark:text-blue-400',
    bg: 'bg-blue-500/15',
    border: 'border-blue-500/20',
  },
  gray: {
    text: 'text-gray-600 dark:text-gray-400',
    bg: 'bg-gray-500/15',
    border: 'border-gray-500/20',
  },
} as const

export type ColorScaleName = keyof typeof colorScaleClasses
