/**
 * Telemetry Data Transformation Utilities
 *
 * Handles time-series data aggregation, time window conversion,
 * and data transformation for chart components.
 */

import type {
  TelemetryAggregate,
  TimeWindowConfig,
  TimeWindowType,
} from '@/types/dashboard'

// ============================================================================
// Time Point Interface
// ============================================================================

export interface TimePoint {
  timestamp: number  // Unix timestamp in seconds
  value: number
}

export interface TimeSeriesData {
  points: TimePoint[]
  sourceId?: string
  metricName?: string
}

// ============================================================================
// Time Window Conversion
// ============================================================================

/**
 * Convert TimeWindowType to hours (for backward compatibility with API).
 *
 * Calendar windows ('today'/'yesterday'/'this_week') are anchored to the same
 * local-day boundaries getTimeRange() queries with — NOT fixed 24h/168h
 * multiples, which leaked neighbouring days into live-event filters near
 * midnight and pulled last week's tail into 'this_week' on non-Mondays.
 */
export function timeWindowToHours(timeWindow: TimeWindowType): number {
  const now = Math.floor(Date.now() / 1000)
  switch (timeWindow) {
    case 'today':
      return Math.max((now - getStartOfDay(now)) / 3600, 1 / 60)
    case 'yesterday':
      // Window is [startOfYesterday, startOfToday]; relative to now that is
      // (elapsed-today + 24h) back.
      return (now - getStartOfDay(now)) / 3600 + 24
    case 'this_week':
      return Math.max((now - getStartOfWeek(now)) / 3600, 1 / 60)
    default:
      break
  }
  const conversions: Record<TimeWindowType, number> = {
    'now': 0,
    'last_5min': 5 / 60,
    'last_15min': 15 / 60,
    'last_30min': 30 / 60,
    'last_1hour': 1,
    'last_6hours': 6,
    'last_24hours': 24,
    'today': 0,   // handled above
    'yesterday': 0, // handled above
    'this_week': 0, // handled above
    'custom': 1,  // Default to 1 hour for custom
  }
  return conversions[timeWindow] ?? 1
}

/**
 * Get time range in seconds from TimeWindowConfig.
 * Returns { start, end } as Unix timestamps.
 */
export function getTimeRange(timeWindow: TimeWindowConfig): { start: number; end: number } {
  const now = Math.floor(Date.now() / 1000)
  const end = timeWindow.endTime ?? now

  if (timeWindow.type === 'custom' && timeWindow.startTime) {
    return { start: timeWindow.startTime, end }
  }

  // Handle 'today', 'yesterday', 'this_week' as absolute time ranges
  // All others are relative offsets from now
  switch (timeWindow.type) {
    case 'now':
      return { start: now, end }
    case 'today':
      return { start: getStartOfDay(now), end }
    case 'yesterday':
      return { start: getStartOfDay(now) - 24 * 60 * 60, end: getStartOfDay(now) }
    case 'this_week':
      return { start: getStartOfWeek(now), end }
    default: {
      // Relative time ranges (offsets from now)
      const offsets: Record<TimeWindowType, number> = {
        'now': 0,
        'last_5min': 5 * 60,
        'last_15min': 15 * 60,
        'last_30min': 30 * 60,
        'last_1hour': 60 * 60,
        'last_6hours': 6 * 60 * 60,
        'last_24hours': 24 * 60 * 60,
        'today': 0,  // Not used here, handled above
        'yesterday': 0,  // Not used here, handled above
        'this_week': 0,  // Not used here, handled above
        'custom': 60 * 60,  // Not used here, custom handled separately
      }
      const offset = offsets[timeWindow.type] ?? 60 * 60
      return { start: now - offset, end }
    }
  }
}

function getStartOfDay(timestamp: number): number {
  const date = new Date(timestamp * 1000)
  return new Date(date.getFullYear(), date.getMonth(), date.getDate()).getTime() / 1000
}

function getStartOfWeek(timestamp: number): number {
  const date = new Date(timestamp * 1000)
  const day = date.getDay()
  const diff = date.getDate() - day + (day === 0 ? -6 : 1)  // Adjust to Monday
  const monday = new Date(date.getFullYear(), date.getMonth(), diff)
  monday.setHours(0, 0, 0, 0)
  // Return absolute timestamp in seconds, NOT offset
  return monday.getTime() / 1000
}

// ============================================================================
// Data Aggregation
// ============================================================================

/**
 * Aggregate time-series data points according to the specified method.
 */
export function aggregateData(
  points: TimePoint[],
  method: TelemetryAggregate
): number | null {
  if (points.length === 0) return null

  const values = points.map(p => p.value).filter(v => v !== null && v !== undefined && !isNaN(v))

  if (values.length === 0) return null

  switch (method) {
    case 'raw':
      // Return all points (special case)
      return points[points.length - 1]?.value ?? null

    case 'latest':
      return values[values.length - 1] ?? null

    case 'first':
      return values[0] ?? null

    case 'avg':
      return values.reduce((sum, v) => sum + v, 0) / values.length

    case 'min':
      return values.reduce((a, b) => Math.min(a, b), Infinity)

    case 'max':
      return values.reduce((a, b) => Math.max(a, b), -Infinity)

    case 'sum':
      return values.reduce((sum, v) => sum + v, 0)

    case 'count':
      return values.length

    case 'delta':
      if (values.length < 2) return 0
      return values[values.length - 1] - values[0]

    case 'rate': {
      if (points.length < 2) return 0
      const first = points[0]
      const last = points[points.length - 1]
      const timeDiff = last.timestamp - first.timestamp
      if (timeDiff <= 0) return 0
      return (last.value - first.value) / timeDiff  // Rate per second
    }

    default:
      return values[values.length - 1] ?? null
  }
}

// ============================================================================
// Data Transformation for Charts
// ============================================================================

/**
 * Format a chart timestamp with automatic date awareness.
 *
 * - If **all** timestamps fall on the same day → show only time (`HH:MM`)
 * - If timestamps span multiple days → show date + time (`MM/DD HH:MM`)
 * - Uses browser locale for formatting.
 *
 * Designed as a factory: call `createChartTimeFormatter(allTimestamps)` once,
 * then use the returned function for each label.
 */
/**
 * Pad a number to 2 digits.
 */
function pad2(n: number): string {
  return n < 10 ? `0${n}` : String(n)
}

export function createChartTimeFormatter(
  timestamps: number[],
  now: Date = new Date()
): (ts: number) => string {
  if (timestamps.length === 0) {
    return () => ''
  }

  // Normalize timestamps to seconds for Date construction
  const dates = timestamps.map(ts => {
    const ms = ts > 10000000000 ? ts : ts * 1000
    return new Date(ms)
  })

  // Check if all timestamps are on the same calendar day
  const daySet = new Set(dates.map(d => d.toDateString()))
  const sameDay = daySet.size === 1
  const allToday = sameDay && dates[0].toDateString() === now.toDateString()

  if (allToday) {
    // Same day, today → just time (HH:mm)
    return (ts: number) => {
      const ms = ts > 10000000000 ? ts : ts * 1000
      const d = new Date(ms)
      return isNaN(d.getTime()) ? String(ts) : `${pad2(d.getHours())}:${pad2(d.getMinutes())}`
    }
  }

  if (sameDay) {
    // Same day, not today → date + time (M/D HH:mm)
    const ref = dates[0]
    const dateStr = `${ref.getMonth() + 1}/${ref.getDate()}`
    return (ts: number) => {
      const ms = ts > 10000000000 ? ts : ts * 1000
      const d = new Date(ms)
      if (isNaN(d.getTime())) return String(ts)
      return `${dateStr} ${pad2(d.getHours())}:${pad2(d.getMinutes())}`
    }
  }

  // Multiple days → always show date + time (M/D HH:mm)
  return (ts: number) => {
    const ms = ts > 10000000000 ? ts : ts * 1000
    const d = new Date(ms)
    if (isNaN(d.getTime())) return String(ts)
    return `${d.getMonth() + 1}/${d.getDate()} ${pad2(d.getHours())}:${pad2(d.getMinutes())}`
  }
}

// ============================================================================
// Helper Functions for DataSource Integration
// ============================================================================

/**
 * Get effective aggregate method from DataSource.
 * After resolveDataSource(), aggregateExt is canonical.
 * Still handles legacy aggregate for DataSources not yet resolved.
 */
export function getEffectiveAggregate(
  dataSource: { aggregate?: string; aggregateExt?: TelemetryAggregate }
): TelemetryAggregate {
  if (dataSource.aggregateExt) {
    return dataSource.aggregateExt
  }

  // Map legacy aggregate values (for un-resolved sources)
  const legacyMap: Record<string, TelemetryAggregate> = {
    'raw': 'raw',
    'avg': 'avg',
    'min': 'min',
    'max': 'max',
    'sum': 'sum',
  }

  return legacyMap[dataSource.aggregate ?? ''] ?? 'raw'
}

/**
 * Get effective time window from DataSource.
 * Handles both legacy `timeRange` and new `timeWindow`.
 */
export function getEffectiveTimeWindow(
  dataSource: { timeRange?: number; timeWindow?: TimeWindowConfig }
): TimeWindowConfig {
  if (dataSource.timeWindow) {
    return dataSource.timeWindow
  }

  // Convert legacy timeRange (hours) to TimeWindowConfig
  const hours = dataSource.timeRange ?? 1
  if (hours === 0) {
    return { type: 'now' }
  }

  // Find closest match — include all relative window types for accurate mapping
  const timeWindowTypes: TimeWindowType[] = [
    'last_5min', 'last_15min', 'last_30min', 'last_1hour', 'last_6hours', 'last_24hours',
  ]
  for (const type of timeWindowTypes) {
    if (timeWindowToHours(type) >= hours) {
      return { type }
    }
  }

  return { type: 'last_24hours' }
}
