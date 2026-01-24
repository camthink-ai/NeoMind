/**
 * Design System - Format Utilities
 *
 * Helper functions for formatting values in dashboard components.
 */

import type { DisplayProps } from '../base/types'

// ============================================================================
// Number Formatting
// ============================================================================

/**
 * Format a number with specified options
 */
export function formatNumber(
  value: number,
  options: {
    precision?: number
    minDecimalPlaces?: number
    maxDecimalPlaces?: number
    locale?: string
    prefix?: string
    suffix?: string
    unit?: string
  } = {}
): string {
  const {
    precision,
    minDecimalPlaces = 0,
    maxDecimalPlaces = 1,
    locale = undefined,
    prefix = '',
    suffix = '',
    unit = '',
  } = options

  // Determine decimal places
  const minimumFractionDigits = precision ?? minDecimalPlaces
  const maximumFractionDigits = precision ?? maxDecimalPlaces

  const formatted = value.toLocaleString(locale, {
    minimumFractionDigits,
    maximumFractionDigits,
  })

  return `${prefix}${formatted}${suffix}${unit}`
}

/**
 * Format a percentage value
 */
export function formatPercentage(
  value: number,
  options: { precision?: number; locale?: string } = {}
): string {
  return formatNumber(value, { ...options, suffix: '%' })
}

/**
 * Format a file size in bytes
 */
export function formatBytes(bytes: number, decimals = 1): string {
  if (bytes === 0) return '0 B'

  const k = 1024
  const dm = decimals < 0 ? 0 : decimals
  const sizes = ['B', 'KB', 'MB', 'GB', 'TB']

  const i = Math.floor(Math.log(bytes) / Math.log(k))

  return `${parseFloat((bytes / Math.pow(k, i)).toFixed(dm))} ${sizes[i]}`
}

/**
 * Format a duration in seconds to human-readable time
 */
export function formatDuration(seconds: number): string {
  if (seconds < 60) {
    return `${Math.round(seconds)}s`
  }
  if (seconds < 3600) {
    const mins = Math.floor(seconds / 60)
    const secs = Math.round(seconds % 60)
    return secs > 0 ? `${mins}m ${secs}s` : `${mins}m`
  }
  const hours = Math.floor(seconds / 3600)
  const mins = Math.round((seconds % 3600) / 60)
  return mins > 0 ? `${hours}h ${mins}m` : `${hours}h`
}

/**
 * Format a timestamp to relative time (e.g., "2 hours ago")
 */
export function formatRelativeTime(timestamp: number | Date): string {
  const date = timestamp instanceof Date ? timestamp : new Date(timestamp)
  const now = new Date()
  const diffMs = now.getTime() - date.getTime()
  const diffSecs = Math.floor(diffMs / 1000)
  const diffMins = Math.floor(diffSecs / 60)
  const diffHours = Math.floor(diffMins / 60)
  const diffDays = Math.floor(diffHours / 24)

  if (diffSecs < 60) return 'just now'
  if (diffMins < 60) return `${diffMins}m ago`
  if (diffHours < 24) return `${diffHours}h ago`
  if (diffDays < 7) return `${diffDays}d ago`

  return date.toLocaleDateString()
}

// ============================================================================
// Value Extraction & Formatting
// ============================================================================

/**
 * Safely extract a displayable value from unknown data
 */
export function toDisplayValue(
  data: unknown,
  fallback: string | number = '--'
): string | number {
  if (data === null || data === undefined) {
    return fallback
  }

  const type = typeof data

  // String - return as-is
  if (type === 'string') {
    const str = data as string
    // Check if it's a numeric string
    if (/^-?\d+\.?\d*$/.test(str.trim())) {
      return str
    }
    return str
  }

  // Number - return as-is
  if (type === 'number') {
    return data as number
  }

  // Boolean - convert to On/Off
  if (type === 'boolean') {
    return data ? 'On' : 'Off'
  }

  // Object - try to extract value property
  if (type === 'object' && data !== null) {
    const dataObj = data as Record<string, unknown>
    if ('value' in dataObj) {
      return toDisplayValue(dataObj.value, fallback)
    }
  }

  return fallback
}

/**
 * Format a value using display options
 */
export function formatValue(
  value: unknown,
  options: DisplayProps & { fallback?: string | number } = {}
): string {
  const { fallback = '--', ...displayOptions } = options

  const displayValue = toDisplayValue(value, fallback)

  // If it's a number, apply formatting
  if (typeof displayValue === 'number') {
    return formatNumber(displayValue, displayOptions)
  }

  // If it's a numeric string, parse and format
  if (typeof displayValue === 'string' && /^-?\d+\.?\d*$/.test(displayValue)) {
    return formatNumber(parseFloat(displayValue), displayOptions)
  }

  return String(displayValue)
}

// ============================================================================
// Color Utilities
// ============================================================================

/**
 * Determine color based on value and thresholds
 */
export interface ColorThreshold {
  value: number
  operator: '>' | '<' | '=' | '>=' | '<='
  color: string
}

export function getColorForValue(
  value: number,
  thresholds: ColorThreshold[],
  defaultColor = 'currentColor'
): string {
  for (const threshold of thresholds) {
    const matches = compare(value, threshold.value, threshold.operator)
    if (matches) {
      return threshold.color
    }
  }
  return defaultColor
}

function compare(a: number, b: number, operator: string): boolean {
  switch (operator) {
    case '>':
      return a > b
    case '<':
      return a < b
    case '=':
      return a === b
    case '>=':
      return a >= b
    case '<=':
      return a <= b
    default:
      return false
  }
}

/**
 * Get status color class based on status name
 */
export function getStatusColorClass(
  status: string
): string {
  const statusLower = status.toLowerCase()

  const statusColors: Record<string, string> = {
    online: 'text-green-500',
    active: 'text-green-500',
    enabled: 'text-green-500',
    success: 'text-green-500',
    completed: 'text-green-500',

    offline: 'text-gray-400',
    inactive: 'text-gray-400',
    disabled: 'text-gray-400',
    unknown: 'text-gray-400',
    idle: 'text-gray-400',

    error: 'text-red-500',
    failed: 'text-red-500',
    danger: 'text-red-500',

    warning: 'text-yellow-500',
    pending: 'text-yellow-500',
    paused: 'text-yellow-500',

    info: 'text-blue-500',
    running: 'text-blue-500',
    loading: 'text-blue-500',
    busy: 'text-blue-500',
  }

  return statusColors[statusLower] || 'text-gray-400'
}

/**
 * Get status background color class
 */
export function getStatusBgClass(status: string): string {
  const statusLower = status.toLowerCase()

  const bgColors: Record<string, string> = {
    online: 'bg-green-500/15',
    active: 'bg-green-500/15',
    success: 'bg-green-500/15',

    offline: 'bg-gray-400/15',
    idle: 'bg-gray-400/15',

    error: 'bg-red-500/15',
    failed: 'bg-red-500/15',

    warning: 'bg-yellow-500/15',
    pending: 'bg-yellow-500/15',

    info: 'bg-blue-500/15',
    running: 'bg-blue-500/15',
    loading: 'bg-blue-500/15',
  }

  return bgColors[statusLower] || 'bg-gray-400/15'
}

// ============================================================================
// Array Utilities
// ============================================================================

/**
 * Safely extract number array from unknown data
 */
export function toNumberArray(data: unknown, fallback: number[] = []): number[] {
  if (Array.isArray(data)) {
    const result: number[] = []
    for (const item of data) {
      if (typeof item === 'number' && !isNaN(item)) {
        result.push(item)
      } else if (typeof item === 'string' && !isNaN(parseFloat(item))) {
        result.push(parseFloat(item))
      }
    }
    return result.length > 0 ? result : fallback
  }
  return fallback
}

/**
 * Get last N values from an array
 */
export function getLastN<T>(arr: T[], n: number): T[] {
  if (arr.length <= n) return arr
  return arr.slice(-n)
}

/**
 * Calculate statistics from number array
 */
export function getArrayStats(arr: number[]) {
  if (arr.length === 0) {
    return { min: 0, max: 0, avg: 0, sum: 0, count: 0 }
  }

  const sum = arr.reduce((a, b) => a + b, 0)
  const min = Math.min(...arr)
  const max = Math.max(...arr)
  const avg = sum / arr.length

  return { min, max, avg, sum, count: arr.length }
}

// ============================================================================
// Validation Utilities
// ============================================================================

/**
 * Check if a value is considered empty
 */
export function isEmpty(value: unknown): boolean {
  if (value === null || value === undefined) return true
  if (typeof value === 'string') return value.trim() === ''
  if (Array.isArray(value)) return value.length === 0
  if (typeof value === 'object') return Object.keys(value as object).length === 0
  return false
}

/**
 * Check if a value is a valid number
 */
export function isValidNumber(value: unknown): value is number {
  return typeof value === 'number' && !isNaN(value) && isFinite(value)
}

/**
 * Clamp a number between min and max
 */
export function clamp(value: number, min: number, max: number): number {
  return Math.max(min, Math.min(max, value))
}

/**
 * Normalize a value to a 0-1 range
 */
export function normalize(value: number, min: number, max: number): number {
  return (value - min) / (max - min || 1)
}
