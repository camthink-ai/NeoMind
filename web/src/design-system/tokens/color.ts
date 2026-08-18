/**
 * Design Tokens - Color
 *
 * Unified color system for all dashboard components.
 * Uses OKLCH for perceptual uniformity.
 */

// Chart colors — SINGLE SOURCE: the `--chart-1..6` CSS variables in
// src/index.css. Keep these values byte-identical with the CSS vars (and the
// hexes below with accurate sRGB conversions of them) — chart series must not
// render a different palette depending on whether a component reads the CSS
// var or this JS constant (SVG attribute contexts can't use var()).
export const chartColors = {
  1: 'oklch(0.65 0.18 300)',   // Purple — primary series
  2: 'oklch(0.65 0.17 155)',   // Emerald — growth/positive
  3: 'oklch(0.72 0.16 85)',    // Amber — warm accent
  4: 'oklch(0.637 0.237 41)',  // Brand orange — energy/alert
  5: 'oklch(0.65 0.17 20)',    // Coral — highlight/attention
  6: 'oklch(0.65 0.12 210)',   // Sky Blue — cool complement
} as const

export type ChartColor = keyof typeof chartColors

// Hex equivalents for SVG rendering (Recharts needs hex, not OKLCH)
// These are accurate sRGB conversions of the OKLCH values above
export const chartColorsHex = [
  '#a170eb', // Purple       (chartColors[1])
  '#00ac5f', // Emerald      (chartColors[2])
  '#d29a00', // Amber        (chartColors[3])
  '#f83c00', // Brand orange (chartColors[4])
  '#e45b63', // Coral        (chartColors[5])
  '#00a2b7', // Sky Blue     (chartColors[6])
] as const

// Status colors — tuned for semantic meaning with good contrast
export const statusColors = {
  success: 'oklch(0.65 0.20 155)',      // Emerald green
  warning: 'oklch(0.72 0.17 65)',       // Amber
  error: 'oklch(0.58 0.22 25)',         // Deep red-orange
  info: 'oklch(0.62 0.22 270)',         // Indigo-blue
  neutral: 'oklch(0.55 0.02 260)',      // Cool gray
} as const

export type StatusColor = keyof typeof statusColors

// Status colors with opacity for backgrounds
export const statusBgColors = {
  success: 'oklch(0.65 0.20 155 / 0.15)',
  warning: 'oklch(0.72 0.17 65 / 0.15)',
  error: 'oklch(0.58 0.22 25 / 0.15)',
  info: 'oklch(0.62 0.22 270 / 0.15)',
  neutral: 'oklch(0.55 0.02 260 / 0.1)',
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
  up: 'oklch(0.65 0.20 155)',       // Emerald
  down: 'oklch(0.58 0.22 25)',      // Deep red-orange
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

// Color scale classes for Tailwind — use semantic theme tokens
export const colorScaleClasses = {
  green: {
    text: 'text-success',
    bg: 'bg-success-light',
    border: 'border-success-light',
  },
  yellow: {
    text: 'text-warning',
    bg: 'bg-warning-light',
    border: 'border-warning-light',
  },
  red: {
    text: 'text-error',
    bg: 'bg-error-light',
    border: 'border-error-light',
  },
  blue: {
    text: 'text-info',
    bg: 'bg-info-light',
    border: 'border-info-light',
  },
  gray: {
    text: 'text-muted-foreground',
    bg: 'bg-muted',
    border: 'border-border',
  },
} as const

export type ColorScaleName = keyof typeof colorScaleClasses
