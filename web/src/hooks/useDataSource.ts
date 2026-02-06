/**
 * useDataSource Hook
 *
 * Simplified data binding for dashboard components.
 * - Efficient telemetry caching with 5-second TTL
 * - Real-time WebSocket event handling
 * - Fuzzy matching for metric value lookup
 * - Store merge for live updates
 */

import { useEffect, useState, useCallback, useRef, useMemo } from 'react'
import type { DataSourceOrList, DataSource, TelemetryAggregate } from '@/types/dashboard'
import { normalizeDataSource } from '@/types/dashboard'
import type { Device } from '@/types'
import type { NeoMindStore } from '@/store'
import { useEvents } from '@/hooks/useEvents'
import { useStore } from '@/store'
import { toNumberArray, isEmpty, isValidNumber } from '@/design-system/utils/format'
import { logError } from '@/lib/errors'

// ============================================================================
// Types
// ============================================================================

export interface UseDataSourceResult<T = unknown> {
  data: T | null
  loading: boolean
  error: string | null
  lastUpdate: number | null
  sendCommand?: (value?: unknown) => Promise<boolean>
  sending?: boolean
}

// ============================================================================
// Global State for Fetch Deduplication
// ============================================================================

const activeFetches = new Map<string, Promise<{ success: boolean; metricsCount: number }>>()
const fetchedDevices = new Set<string>()
const MAX_ACTIVE_FETCHES = 50  // Limit concurrent/pending fetches

// ============================================================================
// Data Fetching
// ============================================================================

/**
 * Fetch device current state with deduplication
 */
async function fetchDeviceTelemetry(deviceId: string): Promise<{ success: boolean; metricsCount: number }> {
  const existingFetch = activeFetches.get(deviceId)
  if (existingFetch) {
    return existingFetch
  }

  // Limit active fetches to prevent memory buildup
  if (activeFetches.size >= MAX_ACTIVE_FETCHES) {
    // Remove oldest entry (first in Map)
    const firstKey = activeFetches.keys().next().value
    if (firstKey) {
      activeFetches.delete(firstKey)
    }
  }

  const fetchPromise = (async () => {
    try {
      const api = (await import('@/lib/api')).api
      const details = await api.getDeviceCurrent(deviceId)

      if (details?.metrics) {
        const store = useStore.getState()
        let updateCount = 0

        Object.entries(details.metrics).forEach(([metricName, metricData]: [string, unknown]) => {
          const value = (metricData as { value?: unknown }).value
          if (value !== null && value !== undefined) {
            store.updateDeviceMetric(deviceId, metricName, value)
            updateCount++
          }
        })

        if (updateCount > 0) {
          fetchedDevices.add(deviceId)
          // Limit fetchedDevices set size
          if (fetchedDevices.size > 200) {
            const firstEntry = fetchedDevices.values().next().value
            if (firstEntry) {
              fetchedDevices.delete(firstEntry)
            }
          }
        }

        return { success: true, metricsCount: updateCount }
      }
      return { success: false, metricsCount: 0 }
    } catch (error) {
      return { success: false, metricsCount: 0 }
    } finally {
      activeFetches.delete(deviceId)
    }
  })()

  activeFetches.set(deviceId, fetchPromise)
  return fetchPromise
}

/**
 * Cache for historical telemetry data
 * Key: deviceId|metric|timeRange|limit|aggregate
 */
interface TelemetryCacheEntry {
  data: number[]
  raw?: unknown[]
  timestamp: number
  refreshing?: boolean  // If a refresh is scheduled
  refreshAfter?: number  // Unix timestamp when refresh should occur
}
const telemetryCache = new Map<string, TelemetryCacheEntry>()
const TELEMETRY_CACHE_TTL = 5000 // 5 seconds cache
const MAX_TELEMETRY_CACHE_SIZE = 50  // Limit cache size to prevent memory issues

/**
 * Cache for system stats data
 */
const systemStatsCache = new Map<string, { data: unknown; timestamp: number }>()
const SYSTEM_CACHE_TTL = 5000 // 5 seconds cache
const MAX_SYSTEM_CACHE_SIZE = 20  // Limit cache size

/**
 * Fetch system stats for a specific metric
 */
async function fetchSystemStats(
  metric: string
): Promise<{ data: unknown; success: boolean }> {
  const cacheKey = `system|${metric}`
  const cached = systemStatsCache.get(cacheKey)

  // Return cached data if fresh
  if (cached && Date.now() - cached.timestamp < SYSTEM_CACHE_TTL) {
    return { data: cached.data, success: true }
  }

  try {
    const api = (await import('@/lib/api')).api
    const stats = await api.getSystemStats()

    if (!stats) {
      return { data: null, success: false }
    }

    // Extract the requested metric
    let value: unknown = null
    switch (metric) {
      case 'uptime':
        value = stats.uptime
        break
      case 'cpu_count':
        value = stats.cpu_count
        break
      case 'total_memory':
        // Convert bytes to GB
        value = stats.total_memory / (1024 * 1024 * 1024)
        break
      case 'used_memory':
        value = stats.used_memory / (1024 * 1024 * 1024)
        break
      case 'free_memory':
        value = stats.free_memory / (1024 * 1024 * 1024)
        break
      case 'available_memory':
        value = stats.available_memory / (1024 * 1024 * 1024)
        break
      case 'memory_percent':
        value = stats.used_memory / stats.total_memory * 100
        break
      case 'platform':
        value = stats.platform
        break
      case 'arch':
        value = stats.arch
        break
      case 'version':
        value = stats.version
        break
      default:
        value = null
    }

    // Cache the result
    systemStatsCache.set(cacheKey, {
      data: value,
      timestamp: Date.now()
    })

    return { data: value, success: true }
  } catch (error) {
    logError(error, { operation: 'Fetch system stats' })
    return { data: null, success: false }
  }
}

/**
 * Fetch historical telemetry data for a device metric
 * @param includeRawPoints - if true, return full TelemetryPoint[] instead of just values
 */
async function fetchHistoricalTelemetry(
  deviceId: string,
  metricId: string,
  timeRange: number = 1, // hours
  limit: number = 50,
  aggregate: TelemetryAggregate = 'raw',
  includeRawPoints: boolean = false,
  bypassCache: boolean = false
): Promise<{ data: number[]; raw?: unknown[]; success: boolean }> {
  const cacheKey = `${deviceId}|${metricId}|${timeRange}|${limit}|${aggregate}`
  const cached = telemetryCache.get(cacheKey)

  // Return cached data if fresh (unless bypassing cache)
  if (!bypassCache && cached && Date.now() - cached.timestamp < TELEMETRY_CACHE_TTL) {
    return { data: cached.data, raw: cached.raw, success: true }
  }

  try {
    const api = (await import('@/lib/api')).api
    const now = Date.now()
    // Ensure minimum time range of 5 minutes for "now" (timeRange = 0) to get at least some data
    const effectiveTimeRange = timeRange > 0 ? timeRange : 5 / 60
    const start = now - effectiveTimeRange * 60 * 60 * 1000

    // Fetch with larger limit for non-raw aggregates to get enough data points
    const fetchLimit = aggregate === 'raw' ? limit : Math.max(limit, 100)
    const startSec = Math.floor(start / 1000)
    const endSec = Math.floor(now / 1000)

    // Fetch telemetry using the exact metricId from device type definition
    // No fallback logic needed since metricId is already the correct key from device type
    const response = await api.getDeviceTelemetry(deviceId, metricId, startSec, endSec, fetchLimit)
    const metricData = response?.data && typeof response.data === 'object' ? (response.data as Record<string, unknown[]>)[metricId] : undefined

    // TelemetryDataResponse has structure: { data: Record<string, TelemetryPoint[]> }
    if (response?.data && typeof response.data === 'object' && Array.isArray(metricData) && metricData.length > 0) {
        // Helper to extract value from a point
        const extractValue = (point: unknown): number => {
          if (typeof point === 'number') return point
          if (typeof point === 'object' && point !== null) {
            const p = point as unknown as Record<string, unknown>
            const rawValue = p.value ?? p.v ?? p.avg ?? p.min ?? p.max ?? 0
            if (typeof rawValue === 'number') return rawValue
            if (typeof rawValue === 'string') {
              const parsed = parseFloat(rawValue)
              return isNaN(parsed) ? 0 : parsed
            }
            if (typeof rawValue === 'boolean') return rawValue ? 1 : 0
            return 0
          }
          return 0
        }

        // Helper to extract timestamp from a point (converts ms to seconds if needed)
        const extractTimestamp = (point: unknown): number => {
          if (typeof point === 'object' && point !== null) {
            const p = point as unknown as Record<string, unknown>
            const ts = p.timestamp ?? p.time ?? p.t
            if (typeof ts === 'number') {
              // Convert milliseconds to seconds if needed (timestamps > 10000000000 are in ms)
              return ts > 10000000000 ? Math.floor(ts / 1000) : ts
            }
          }
          return Date.now() / 1000
        }

        // Extract all values
        const allValues = metricData.map(extractValue).filter((v: number) => typeof v === 'number' && !isNaN(v))

        // Apply aggregation
        // NOTE: API returns data in DESCENDING order (newest first: index 0 = newest)
        let values: number[]
        let rawPoints: unknown[] | undefined

        if (aggregate === 'latest') {
          // Return only the newest value (index 0 in descending order)
          const newValue = allValues[0] ?? 0
          values = [newValue]
          // Create a single raw point with the original value (could be string, number, etc.)
          if (includeRawPoints) {
            const newPoint = metricData[0]
            // Extract the raw value preserving its type
            const rawValue = typeof newPoint === 'object' && newPoint !== null
              ? (newPoint as unknown as Record<string, unknown>).value ?? (newPoint as unknown as Record<string, unknown>).v ?? newPoint
              : newPoint
            rawPoints = [{
              timestamp: extractTimestamp(newPoint),
              value: rawValue,
            }]
          }
        } else if (aggregate === 'first') {
          // Return only the oldest value (last index in descending order)
          const oldValue = allValues[allValues.length - 1] ?? 0
          values = [oldValue]
          if (includeRawPoints) {
            const oldPoint = metricData[metricData.length - 1]
            const rawValue = typeof oldPoint === 'object' && oldPoint !== null
              ? (oldPoint as unknown as Record<string, unknown>).value ?? (oldPoint as unknown as Record<string, unknown>).v ?? oldPoint
              : oldPoint
            rawPoints = [{
              timestamp: extractTimestamp(oldPoint),
              value: rawValue,
            }]
          }
        } else if (aggregate === 'avg') {
          // Calculate average
          const sum = allValues.reduce((a, b) => a + b, 0)
          const avg = allValues.length > 0 ? sum / allValues.length : 0
          values = [avg]
        } else if (aggregate === 'min') {
          // Return minimum value
          const min = Math.min(...allValues)
          values = [min]
        } else if (aggregate === 'max') {
          // Return maximum value
          const max = Math.max(...allValues)
          values = [max]
        } else if (aggregate === 'sum') {
          // Return sum
          const sum = allValues.reduce((a, b) => a + b, 0)
          values = [sum]
        } else if (aggregate === 'delta') {
          // Return change (newest - oldest)
          // With descending order: allValues[0] = newest, allValues[length-1] = oldest
          const newValue = allValues[0] ?? 0
          const oldValue = allValues[allValues.length - 1] ?? 0
          values = [newValue - oldValue]
        } else if (aggregate === 'count') {
          // Return count
          values = [allValues.length]
        } else {
          // 'raw' or unknown: return all values
          values = allValues
          // For raw points, preserve original structure including string values
          rawPoints = includeRawPoints ? metricData.map((point, index) => {
            if (typeof point === 'number') {
              // Pure number value - use a placeholder timestamp since we don't have the actual point timestamp
              // This shouldn't happen in normal telemetry responses
              return { timestamp: Date.now() / 1000, value: point }
            }
            if (typeof point === 'object' && point !== null) {
              const p = point as unknown as Record<string, unknown>
              let ts = p.timestamp ?? p.time ?? p.t
              let timestamp: number
              const originalTs = ts
              if (typeof ts === 'number') {
                // Convert milliseconds to seconds if needed (timestamps > 10000000000 are in ms)
                timestamp = ts > 10000000000 ? Math.floor(ts / 1000) : ts
              } else {
                timestamp = Date.now() / 1000
              }
              const value = p.value ?? p.v ?? 0

              return { timestamp, value }
            }
            return { timestamp: Date.now() / 1000, value: point }
          }) : undefined
        }

        // Cache the result
        telemetryCache.set(cacheKey, {
          data: values,
          raw: rawPoints,
          timestamp: Date.now()
        })

        return { data: values, raw: rawPoints, success: true }
    }

    return { data: [], success: false }
  } catch (error) {
    logError(error, { operation: 'Fetch historical telemetry' })
    return { data: [], success: false }
  }
}

/** Window (seconds) to treat points as near-duplicates when value is the same */
const TELEMETRY_DEDUP_WINDOW_SEC = 5

/**
 * Check if a point is image data (has src, url, or value field with base64/URL content)
 */
function isImageData(p: unknown): boolean {
  if (p == null || typeof p !== 'object') return false
  const o = p as Record<string, unknown>
  // Check for transformed format (src, url)
  if ('src' in o || 'url' in o) return true
  // Check for raw telemetry format (value with base64 or URL)
  if ('value' in o || 'v' in o) {
    const val = (o.value ?? o.v) as string | undefined
    if (typeof val === 'string') {
      // Base64 image data or HTTP URL
      const isImage = val.startsWith('data:image/') ||
             val.startsWith('data:base64,') ||
             val.startsWith('http') ||
             val.startsWith('/') ||
             val.length > 2000  // Long base64 string (images)
      return isImage
    }
  }
  // Check for data field with long string (base64)
  if ('data' in o && typeof o.data === 'string' && o.data.length > 100) {
    return true
  }
  return false
}

/**
 * Get value from a telemetry point (handles {value}, {v}, etc.)
 */
function getPointValue(p: unknown): unknown {
  if (p == null) return undefined
  const o = p as Record<string, unknown>
  return o.value ?? o.v ?? o.val
}

/**
 * Deduplicate telemetry points: keep one per (timestamp, value) with near-duplicate collapse.
 * Points with same value within DEDUP_WINDOW_SEC are treated as duplicates (fixes MQTT+DeviceService double-write).
 * For image data, skip deduplication entirely (each image is unique, keep all).
 */
function dedupeTelemetryPoints(
  sorted: unknown[],
  getTs: (p: unknown) => number,
  maxLimit: number
): unknown[] {
  // Check if this batch contains any image data
  const hasImageData = sorted.some(isImageData)

  // If all data is images, skip deduplication entirely
  if (hasImageData) {
    return sorted.slice(0, maxLimit)
  }

  // Normal deduplication for non-image data
  const deduped: unknown[] = []
  for (const p of sorted) {
    const ts = getTs(p)
    const val = getPointValue(p)

    // Exact timestamp duplicate - always remove
    const exactDup = deduped.some((k) => getTs(k) === ts)
    if (exactDup) continue

    // Near-duplicate: same value, within window (handles double-write from backend)
    const nearDup = deduped.some((k) => {
      const kVal = getPointValue(k)
      const kTs = getTs(k)
      const valueEqual =
        val === kVal ||
        (typeof val === 'object' && typeof kVal === 'object' && val != null && kVal != null &&
          JSON.stringify(val) === JSON.stringify(kVal))
      return valueEqual && Math.abs(ts - kTs) <= TELEMETRY_DEDUP_WINDOW_SEC
    })
    if (nearDup) continue
    deduped.push(p)
    if (deduped.length >= maxLimit) break
  }
  return deduped
}

/**
 * Clear expired telemetry cache entries
 * Also enforces cache size limits
 */
function cleanupTelemetryCache() {
  const now = Date.now()

  // Clean expired telemetry cache entries
  for (const [key, value] of telemetryCache.entries()) {
    if (now - value.timestamp > TELEMETRY_CACHE_TTL) {
      telemetryCache.delete(key)
    }
  }

  // Enforce telemetry cache size limit (remove oldest entries if needed)
  if (telemetryCache.size > MAX_TELEMETRY_CACHE_SIZE) {
    const entriesToRemove = telemetryCache.size - MAX_TELEMETRY_CACHE_SIZE
    let removed = 0
    for (const key of telemetryCache.keys()) {
      if (removed >= entriesToRemove) break
      telemetryCache.delete(key)
      removed++
    }
  }

  // Clean expired system stats cache entries
  for (const [key, value] of systemStatsCache.entries()) {
    if (now - value.timestamp > SYSTEM_CACHE_TTL) {
      systemStatsCache.delete(key)
    }
  }

  // Enforce system stats cache size limit
  if (systemStatsCache.size > MAX_SYSTEM_CACHE_SIZE) {
    const entriesToRemove = systemStatsCache.size - MAX_SYSTEM_CACHE_SIZE
    let removed = 0
    for (const key of systemStatsCache.keys()) {
      if (removed >= entriesToRemove) break
      systemStatsCache.delete(key)
      removed++
    }
  }
}

// Periodic cache cleanup - store reference for potential cleanup
let cacheCleanupInterval: ReturnType<typeof setInterval> | null = null
if (typeof window !== 'undefined') {
  cacheCleanupInterval = setInterval(cleanupTelemetryCache, 60000) // Clean up every minute
}

/**
 * Clear all global cache intervals (call on app unmount)
 */
export function clearGlobalCacheIntervals() {
  if (cacheCleanupInterval) {
    clearInterval(cacheCleanupInterval)
    cacheCleanupInterval = null
  }
}

// ============================================================================
// Data Extraction Utilities
// ============================================================================

/**
 * Safely extract a value from unknown data
 */
function safeExtractValue(data: unknown, fallback: number | string | boolean = 0): unknown {
  if (data === null || data === undefined) return fallback
  const type = typeof data

  if (type === 'string' || type === 'number' || type === 'boolean') return data

  if (typeof data === 'object' && data !== null) {
    if ('value' in data) {
      return safeExtractValue((data as { value: unknown }).value, fallback)
    }
    return data
  }

  return fallback
}

/**
 * Find property value with various naming conventions
 */
function findPropertyValue(obj: Record<string, unknown>, property: string): unknown {
  if (property in obj) return obj[property]

  const lowerProp = property.toLowerCase()
  for (const key of Object.keys(obj)) {
    if (key.toLowerCase() === lowerProp) return obj[key]
  }

  // Common aliases
  const aliases: Record<string, string[]> = {
    temperature: ['temperature', 'temp', 'value', 'temp_c', 'tempC'],
    humidity: ['humidity', 'hum', 'rh', 'relative_humidity'],
    status: ['status', 'state', 'connection_status', 'online'],
    value: ['value', 'val', 'current', 'presentValue', 'pv'],
  }

  for (const [key, aliasList] of Object.entries(aliases)) {
    if (lowerProp === key || lowerProp === key.slice(0, -1)) {
      for (const alias of aliasList) {
        if (alias in obj) return obj[alias]
      }
    }
  }

  return undefined
}

/**
 * Check if event metric matches widget metricId (supports nested paths like values.image vs image).
 * This handles both directions:
 * - Event "values.image" matches widget "image" (event is nested, widget is simple)
 * - Event "image" matches widget "values.image" (event is simple, widget is nested)
 */
function eventMetricMatches(eventMetric: string, widgetMetricId: string): boolean {
  if (!eventMetric || !widgetMetricId) return false
  if (eventMetric === widgetMetricId) return true

  // Case 1: Event has nested path, widget is simple
  // e.g., event "values.image" matches widget "image"
  if (eventMetric.endsWith('.' + widgetMetricId)) return true
  if (eventMetric.endsWith('/' + widgetMetricId)) return true

  // Case 2: Event is simple, widget has nested path
  // e.g., event "image" matches widget "values.image"
  if (widgetMetricId.endsWith('.' + eventMetric)) return true
  if (widgetMetricId.endsWith('/' + eventMetric)) return true

  // Case 3: Both have nested paths - compare the last segment
  const eventLastSegment = eventMetric.split('.').pop() || eventMetric.split('/').pop() || eventMetric
  const widgetLastSegment = widgetMetricId.split('.').pop() || widgetMetricId.split('/').pop() || widgetMetricId
  if (eventLastSegment === widgetLastSegment) return true

  return false
}

/**
 * Extract value from a parsed JSON object using dot notation.
 * Helper for extractValueFromData to handle _raw events.
 */
function extractValueFromParsed(data: unknown, property: string): unknown {
  if (data === null || data === undefined) return undefined
  if (typeof data !== 'object') return data

  const dataObj = data as Record<string, unknown>

  // Direct key match
  if (property in dataObj) return dataObj[property]

  // Dot notation for nested paths
  if (property.includes('.')) {
    const parts = property.split('.')
    let current: unknown = dataObj

    for (let i = 0; i < parts.length; i++) {
      const part = parts[i]
      if (typeof current === 'object' && current !== null && part in current) {
        current = (current as Record<string, unknown>)[part]
        if (i === parts.length - 1 || typeof current !== 'object') return current
      } else {
        return undefined
      }
    }
    return current
  }

  // Case-insensitive match
  const lowerProp = property.toLowerCase()
  for (const key of Object.keys(dataObj)) {
    if (key.toLowerCase() === lowerProp) return dataObj[key]
  }

  return undefined
}

/**
 * Extract value from nested object using dot notation.
 * Dotted keys like "xx.ss.xx" are tried as a single string key first, then as nested path.
 *
 * Special handling for "_raw" events: if data.value is a JSON string, parse it first.
 */
function extractValueFromData(data: unknown, property: string): unknown {
  if (data === null || data === undefined) return undefined

  // Handle strings directly (return as-is for base64 images, etc.)
  if (typeof data !== 'object') return data

  const dataObj = data as Record<string, unknown>

  // Special handling for "_raw" style events where value is a JSON string
  if ('value' in dataObj && typeof dataObj.value === 'string' && 'metric' in dataObj) {
    const metric = dataObj.metric as string
    // If this is a "_raw" metric event, try to parse the JSON value
    if (metric === '_raw' || (dataObj.value as string).trim().startsWith('{')) {
      try {
        const parsed = JSON.parse(dataObj.value as string)
        // Extract from the parsed JSON
        const extracted = extractValueFromParsed(parsed, property)
        if (extracted !== undefined) return extracted
      } catch {
        // Not valid JSON, continue with normal extraction
      }
    }
  }

  // Prefer direct access so "xx.ss.xx" is treated as a single string key when present
  if (property in dataObj) return dataObj[property]

  // Dot notation for nested paths (e.g. values.image or obj.a.b)
  if (property.includes('.')) {
    const parts = property.split('.')
    let current: unknown = dataObj

    for (let i = 0; i < parts.length; i++) {
      const part = parts[i]
      if (typeof current === 'object' && current !== null && part in current) {
        current = (current as Record<string, unknown>)[part]
        if (i === parts.length - 1 || typeof current !== 'object') return current
      } else {
        // Try flexible matching
        if (typeof current === 'object' && current !== null) {
          const found = findPropertyValue(current as Record<string, unknown>, part)
          if (found !== undefined) {
            current = found
            if (i === parts.length - 1 || typeof current !== 'object') return current
          }
        }
        return undefined
      }
    }
    return current
  }

  // Flexible matching
  const found = findPropertyValue(dataObj, property)
  if (found !== undefined) return found

  // Try nested in common properties
  for (const nestedProp of ['current_values', 'currentValues', 'metrics', 'data', 'values', 'device_info', 'deviceInfo']) {
    if (nestedProp in dataObj && typeof dataObj[nestedProp] === 'object') {
      const nested = dataObj[nestedProp] as Record<string, unknown>
      if (property.includes('.')) {
        const remainingParts = property.split('.')
        if (remainingParts[0].toLowerCase() === nestedProp.toLowerCase()) {
          return extractValueFromData(nested, remainingParts.slice(1).join('.'))
        }
      }
      const nestedValue = findPropertyValue(nested, property)
      if (nestedValue !== undefined) return nestedValue
    }
  }

  return undefined
}

// ============================================================================
// Main Hook
// ============================================================================

/**
 * Shallow comparison for current_values objects
 * Returns true if values are different, false if they're the same
 * Much faster than JSON.stringify for large objects
 */
function hasCurrentValuesChanged(
  current: Record<string, unknown> | undefined | null,
  prev: Record<string, unknown> | undefined | null
): boolean {
  // Quick reference check - most common case
  if (current === prev) return false

  // Both are undefined/null
  if (!current && !prev) return false

  // One is undefined/null, other isn't
  if (!current || !prev) return true

  // Check key count first (fast rejection without allocations)
  const currentKeys = Object.keys(current)
  const prevKeys = Object.keys(prev)
  if (currentKeys.length !== prevKeys.length) return true

  // Check each key's value reference
  for (let i = 0; i < currentKeys.length; i++) {
    const key = currentKeys[i]
    if (current[key] !== prev[key]) return true
  }

  return false
}

/**
 * Helper function to create stable JSON key for memoization
 * Handles objects with potentially different property order
 */
function createStableKey(obj: unknown): string {
  if (obj === null || obj === undefined) return ''
  if (typeof obj !== 'object') return String(obj)
  if (Array.isArray(obj)) return '[' + obj.map(createStableKey).join(',') + ']'
  const sortedKeys = Object.keys(obj).sort()
  const recordObj = obj as Record<string, unknown>
  return '{' + sortedKeys.map(k => `"${k}":${createStableKey(recordObj[k])}`).join(',') + '}'
}

export function useDataSource<T = unknown>(
  dataSource: DataSourceOrList | undefined,
  options?: {
    enabled?: boolean
    transform?: (data: unknown) => T
    fallback?: T
    preserveMultiple?: boolean  // If true, keep multiple data sources separate instead of merging
  }
): UseDataSourceResult<T> {
  const { enabled = true, transform, fallback, preserveMultiple = false } = options ?? {}

  // Normalize data source immediately to check if we have any
  const hasDataSourceValue = dataSource !== undefined &&
                             dataSource !== null &&
                             (Array.isArray(dataSource) ? dataSource.length > 0 : true)

  const [data, setData] = useState<T | null>(fallback ?? null)
  // Start with loading=false if there's no data source or we're disabled
  const [loading, setLoading] = useState(!enabled || !hasDataSourceValue ? false : true)
  const [error, setError] = useState<string | null>(null)
  const [lastUpdate, setLastUpdate] = useState<number | null>(null)
  const [sending, setSending] = useState(false)
  // Telemetry refresh trigger - incremented when WebSocket event matches telemetry source
  const [telemetryRefreshTrigger, setTelemetryRefreshTrigger] = useState(0)

  // Track interval with ref to prevent leaks when dataSources change
  const intervalRef = useRef<ReturnType<typeof setInterval> | null>(null)
  // Track telemetry refresh timer at component level (replaces global window._telemetryRefreshTimer)
  const telemetryRefreshTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null)

  // CRITICAL: Memoize dataSources to prevent infinite re-renders
  // Using stable key generation ensures consistency
  const dataSourceKey = useMemo(() => {
    return createStableKey(dataSource)
  }, [dataSource])

  const dataSources = useMemo(() => {
    return dataSource ? normalizeDataSource(dataSource) : []
  }, [dataSourceKey])

  // Memoize relevant device IDs for optimized store subscription filtering
  const relevantDeviceIds = useMemo(() => {
    const ids = new Set(
      dataSources
        .map((ds) =>
          ds.type === 'device' || ds.type === 'command' || ds.type === 'telemetry' || ds.type === 'device-info'
            ? ds.deviceId
            : null
        )
        .filter(Boolean) as string[]
    )
    return ids
  }, [dataSources])

  // Memoize device-info device IDs for status change tracking
  const deviceInfoIds = useMemo(() => {
    return new Set(
      dataSources
        .filter((ds) => ds.type === 'device-info')
        .map((ds) => ds.deviceId)
        .filter(Boolean) as string[]
    )
  }, [dataSources])

  const initialFetchDoneRef = useRef<Set<string>>(new Set())
  const lastValidDataRef = useRef<Record<string, unknown>>({})

  const optionsRef = useRef({ enabled, transform, fallback, preserveMultiple })
  optionsRef.current = { enabled, transform, fallback, preserveMultiple }

  const dataSourcesRef = useRef(dataSources)
  dataSourcesRef.current = dataSources

  // Ref to track if initial telemetry fetch has completed
  // This prevents showing loading state on refreshes/updates
  const initialTelemetryFetchDoneRef = useRef(false)

  // Ref to track previous telemetry key to detect config changes
  const prevTelemetryKeyRef = useRef<string>('')

  // Ref to track if initial system fetch has completed
  const initialSystemFetchDoneRef = useRef(false)

  // Zustand subscribe only passes (state), not (state, prevState). Keep previous state for comparison.
  const prevStoreStateRef = useRef<{ devices: NeoMindStore['devices'] } | null>(null)

  // Track processed event IDs to prevent duplicate processing
  const processedEventsRef = useRef<Set<string>>(new Set())
  const lastProcessedEventCountRef = useRef(0)

  // Ref to read current data from inside store subscribe (for telemetry merge from store)
  const dataRef = useRef<T | null>(null)
  dataRef.current = data

  // Check for command source
  const hasCommandSource = dataSources.some((ds) => ds.type === 'command')
  const commandSource = dataSources.find((ds) => ds.type === 'command')

  // Send command function - fire and forget, does not update local state
  const sendCommand = useCallback(async (value?: unknown): Promise<boolean> => {
    if (!commandSource || !enabled) return false

    setSending(true)
    setError(null)

    try {
      const deviceId = commandSource.deviceId
      const command = commandSource.command || 'setValue'

      let params: Record<string, unknown> = { value }

      if (commandSource.valueMapping && value !== undefined) {
        const mapping = commandSource.valueMapping
        if (value === true || value === 'on' || value === 1) {
          params = mapping.on !== undefined ? { value: mapping.on } : { value }
        } else if (value === false || value === 'off' || value === 0) {
          params = mapping.off !== undefined ? { value: mapping.off } : { value }
        } else {
          params = mapping.true !== undefined ? { value: mapping.true } : { value }
        }
      }

      if (commandSource.commandParams) {
        params = { ...params, ...commandSource.commandParams }
      }

      const { api } = await import('@/lib/api')
      await api.sendCommand(deviceId!, command, params)

      // Don't update local state - let device telemetry update the state
      // setLastUpdate(Date.now())
      return true
    } catch (err) {
      const errorMessage = err instanceof Error ? err.message : 'Command failed'
      setError(errorMessage)
      return false
    } finally {
      setSending(false)
    }
  }, [commandSource, enabled])

  /**
   * Read data from store (WebSocket populated, no polling)
   */
  const readDataFromStore = useCallback(() => {
    const { transform: transformFn, fallback: fallbackVal } = optionsRef.current
    const currentDataSources = dataSourcesRef.current

    const storeState = useStore.getState()
    const currentDevices = storeState.devices

    if (currentDataSources.length === 0) {
      if (fallbackVal !== undefined) setData(fallbackVal)
      setLoading(false)
      return
    }

    try {
      // Filter out telemetry and system sources - they are handled separately by fetch effects
      const nonTelemetrySources = currentDataSources.filter((ds) => ds.type !== 'telemetry' && ds.type !== 'system')

      // Only process non-telemetry sources here
      const results = nonTelemetrySources.map((ds) => {
        let result: unknown

        switch (ds.type) {
          case 'static':
            result = ds.staticValue
            break

          case 'device': {
            const deviceId = ds.deviceId!
            const property = ds.property as string | undefined
            const device = currentDevices.find((d: Device) => d.id === deviceId || d.device_id === deviceId)

            // If no property specified, return full device object (for map markers, etc.)
            if (!property) {
              result = device ?? null
              break
            }

            // Otherwise extract specific property from current_values
            const cacheKey = `${deviceId}:${property}`

            if (device?.current_values && typeof device.current_values === 'object' && Object.keys(device.current_values).length > 0) {
              const extracted = extractValueFromData(device.current_values, property)
              if (extracted !== undefined) {
                result = extracted
                lastValidDataRef.current[cacheKey] = extracted
              } else {
                // Try nested paths
                let foundNested = false
                for (const nestedKey of ['values', 'metrics', 'data']) {
                  if (device.current_values[nestedKey] && typeof device.current_values[nestedKey] === 'object') {
                    const nestedValue = extractValueFromData(device.current_values[nestedKey] as Record<string, unknown>, property)
                    if (nestedValue !== undefined) {
                      result = nestedValue
                      foundNested = true
                      lastValidDataRef.current[cacheKey] = nestedValue
                      break
                    }
                  }
                }
                if (!foundNested) {
                  result = lastValidDataRef.current[cacheKey] ?? '-'
                }
              }
            } else if (device) {
              // Device exists but no current_values yet
              if (initialFetchDoneRef.current.has(deviceId) || fetchedDevices.has(deviceId) || activeFetches.has(deviceId)) {
                result = lastValidDataRef.current[cacheKey] ?? '-'
              } else {
                initialFetchDoneRef.current.add(deviceId)
                fetchDeviceTelemetry(deviceId).catch(() => {})
                result = lastValidDataRef.current[cacheKey] ?? '-'
              }
            } else {
              // Device not found in store
              if (initialFetchDoneRef.current.has(deviceId) || activeFetches.has(deviceId)) {
                result = lastValidDataRef.current[cacheKey] ?? '-'
              } else {
                initialFetchDoneRef.current.add(deviceId)
                import('@/lib/api').then(({ api }) => {
                  api.getDevices()
                    .then(() => fetchDeviceTelemetry(deviceId))
                    .catch(() => {})
                })
                result = lastValidDataRef.current[cacheKey] ?? '-'
              }
            }

            result = safeExtractValue(result, '-')
            break
          }

          case 'metric': {
            const metricId = ds.metricId ?? 'value'

            for (const device of currentDevices) {
              if (device.current_values && typeof device.current_values === 'object') {
                const value = extractValueFromData(device.current_values, metricId)
                if (value !== undefined) {
                  result = value
                  break
                }
              }
            }

            if (result === undefined) {
              result = fallbackVal ?? '-'
            }
            result = safeExtractValue(result, '-')
            break
          }

          case 'command': {
            const deviceId = ds.deviceId
            const property = ds.property || 'state'
            const device = currentDevices.find((d: Device) => d.id === deviceId)

            if (device?.current_values && typeof device.current_values === 'object') {
              const extracted = extractValueFromData(device.current_values, property)
              result = extracted !== undefined ? extracted : false
            } else {
              result = false
            }

            result = safeExtractValue(result, false)
            break
          }

          case 'device-info': {
            const deviceId = ds.deviceId
            const infoProperty = ds.infoProperty || 'name'
            const device = currentDevices.find((d: Device) => d.id === deviceId || d.device_id === deviceId)

            if (!device) {
              result = fallbackVal ?? '-'
            } else {
              switch (infoProperty) {
                case 'name':
                  result = device.name || '-'
                  break
                case 'status':
                  result = device.status || 'unknown'
                  break
                case 'online':
                  result = device.online ?? false
                  break
                case 'last_seen':
                  result = device.last_seen || '-'
                  break
                case 'device_type':
                  result = device.device_type || '-'
                  break
                case 'plugin_name':
                  result = device.plugin_name || device.adapter_id || '-'
                  break
                case 'adapter_id':
                  result = device.adapter_id || '-'
                  break
                default:
                  result = fallbackVal ?? '-'
              }
            }
            result = safeExtractValue(result as unknown, (fallbackVal ?? '-') as any)
            break
          }

          case 'api':
          case 'websocket':
            result = fallbackVal ?? 0
            break

          case 'computed': {
            const expression = (ds.params?.expression as string) || '0'
            try {
              const tokens = expression.match(/(\d+\.?\d*|[+\-*/])/g)
              if (tokens) {
                 
                result = new Function('return ' + tokens.join(' '))()
              } else {
                result = 0
              }
            } catch {
              result = 0
            }
            result = safeExtractValue(result, 0)
            break
          }

          default:
            result = fallbackVal ?? null
        }

        return result
      })

      // Combine results
      let finalData: unknown
      if (nonTelemetrySources.length > 0 && currentDataSources.length > 1) {
        // Multiple sources, some might be telemetry
        finalData = results
      } else if (nonTelemetrySources.length === 1) {
        // Single non-telemetry source
        finalData = results[0]
      } else if (currentDataSources.length === 0) {
        // No data sources configured
        return
      } else {
        // All sources are telemetry or system - their effects will handle this
        // For single telemetry/system sources, their effects set the data
        return
      }

      const transformedData = transformFn ? transformFn(finalData) : (finalData as T)
      setData(transformedData)
      setLastUpdate(Date.now())
    } catch (err) {
      const errorMessage = err instanceof Error ? err.message : 'Unknown error'
      setError(errorMessage)
      const fallbackData = optionsRef.current.fallback ?? 0
      setData(fallbackData as T)
    } finally {
      setLoading(false)
    }
  }, [])

  // Subscribe to store changes
  useEffect(() => {
    if (dataSources.length === 0) {
      // No data sources - ensure fallback data is set and loading is cleared
      const { fallback: fallbackVal } = optionsRef.current
      if (fallbackVal !== undefined) {
        setData(fallbackVal)
      }
      setLoading(false)
      return
    }

    if (!enabled) {
      setLoading(false)
      return
    }

    // Early exit if no relevant devices to watch
    if (relevantDeviceIds.size === 0) {
      readDataFromStore()
      setLoading(false)
      return
    }

    readDataFromStore()
    prevStoreStateRef.current = { devices: useStore.getState().devices }

    let unsubscribed = false
    // Zustand subscribe only passes (state); we keep prev state in a ref for comparison
    const unsubscribe = useStore.subscribe((state: NeoMindStore) => {
      if (unsubscribed) return

      const prev = prevStoreStateRef.current
      if (!prev) return

      // Fast path: check if devices array reference changed (common case)
      const devicesChanged = state.devices !== prev.devices
      const devicesLengthChanged = state.devices.length !== prev.devices.length

      let currentValuesChanged = false

      if (!devicesLengthChanged) {
        // Build device lookup maps for O(1) access instead of O(n) find()
        // This is especially important when there are many devices
        const stateDeviceMap = new Map<string, Device>()
        const prevDeviceMap = new Map<string, Device>()

        // Only populate maps with devices we care about
        for (const deviceId of relevantDeviceIds) {
          const stateDev = state.devices.find((d: Device) => d.id === deviceId || d.device_id === deviceId)
          const prevDev = prev.devices.find((d: Device) => d.id === deviceId || d.device_id === deviceId)
          if (stateDev) stateDeviceMap.set(deviceId, stateDev)
          if (prevDev) prevDeviceMap.set(deviceId, prevDev)
        }

        // Check only relevant devices
        for (const deviceId of relevantDeviceIds) {
          const device = stateDeviceMap.get(deviceId)
          const prevDevice = prevDeviceMap.get(deviceId)

          if (device && prevDevice) {
            // Use shallow comparison - much faster than JSON.stringify
            if (hasCurrentValuesChanged(device.current_values as Record<string, unknown> | null, prevDevice.current_values as Record<string, unknown> | null)) {
              const hasDataNow = device.current_values && Object.keys(device.current_values).length > 0
              if (hasDataNow) {
                currentValuesChanged = true
                break
              }
            }

            // Check device-info properties (status, online, last_seen) for device-info sources
            if (deviceInfoIds.has(deviceId)) {
              if (device.status !== prevDevice.status ||
                  device.online !== prevDevice.online ||
                  device.last_seen !== prevDevice.last_seen) {
                currentValuesChanged = true
                break
              }
            }
          } else if (device && !prevDevice) {
            // New device appeared
            if (device.current_values && Object.keys(device.current_values).length > 0) {
              currentValuesChanged = true
              break
            }
          } else if (!device && prevDevice) {
            // Device was removed - might need to update
            currentValuesChanged = true
            break
          }
        }
      }

      // Only update prev if something actually changed (avoid unnecessary updates)
      if (devicesChanged || devicesLengthChanged || currentValuesChanged) {
        prevStoreStateRef.current = { devices: state.devices }
        readDataFromStore()

        // For telemetry-only sources (e.g. image history), readDataFromStore skips them.
        // Merge latest from store so UI updates when device current_values change (e.g. from event).
        const currentDataSources = dataSourcesRef.current
        const telemetrySources = currentDataSources.filter((ds) => ds.type === 'telemetry')
        if (telemetrySources.length > 0) {
          const currentData = dataRef.current as unknown
          const now = Math.floor(Date.now() / 1000)
          const getTs = (p: unknown): number => {
            if (p == null) return 0
            const o = p as Record<string, unknown>
            return (o.timestamp ?? o.time ?? o.t ?? 0) as number
          }

          const results = currentDataSources.map((ds, index) => {
            if (ds.type !== 'telemetry') return undefined
            const deviceId = ds.deviceId!
            const metricId = ds.metricId || ds.property || 'value'
            const device = state.devices.find((d: Device) => d.id === deviceId || d.device_id === deviceId)
            const latestValue = device?.current_values
              ? extractValueFromData(device.current_values, metricId)
              : undefined
            if (latestValue === undefined) return undefined

            // Use event timestamp if available for accurate time tracking
            // For telemetry merge, we use current time since this is from store (not direct event)
            // The store value is already the latest, so we timestamp it as "now"
            const newPoint = { timestamp: now, time: now, value: latestValue }

            // Get current array for this source, handling both preserveMultiple and single source cases
            const isPreserveMultiple = optionsRef.current.preserveMultiple
            let currentArray: unknown[] = []
            if (Array.isArray(currentData)) {
              if (isPreserveMultiple && currentDataSources.length > 1) {
                // Multi-source with preserveMultiple: expect nested array structure
                if (Array.isArray(currentData[index])) {
                  currentArray = currentData[index] as unknown[]
                } else if (currentData[index] !== undefined) {
                  // Edge case: data exists but isn't an array, wrap it
                  currentArray = [currentData[index]]
                }
                // If currentData[index] is undefined, leave as empty array - will be initialized
              } else if (currentDataSources.length === 1 || !isPreserveMultiple) {
                // Single source or not preserving multiple: use flat array
                currentArray = currentData as unknown[]
              }
            }
            const merged = [newPoint, ...currentArray]

            // Detect image data sources for special handling (no dedup, higher limit)
            const isImageDataSource = (ds.params?.includeRawPoints === true || ds.transform === 'raw') ||
                                     (metricId && (metricId.toLowerCase().includes('image') ||
                                                    metricId.toLowerCase().includes('img') ||
                                                    metricId.includes('values.image')))
            const maxLimit = isImageDataSource ? 200 : (ds.limit ?? 50)

            const sorted = [...merged].sort((a, b) => getTs(b) - getTs(a))

            // For image data sources, skip deduplication to preserve all unique images
            if (isImageDataSource) {
              return sorted.slice(0, maxLimit)
            }
            return dedupeTelemetryPoints(sorted, getTs, maxLimit)
          })

          if (results.some((r) => r !== undefined)) {
            const finalData = currentDataSources.length > 1
              ? results.map((r, i) => (r !== undefined ? r : (Array.isArray(currentData) && currentData[i] !== undefined ? currentData[i] : [])))
              : results[0] ?? dataRef.current
            const { transform: transformFn } = optionsRef.current
            const transformed = transformFn ? transformFn(finalData) : (finalData as T)
            setData(transformed)
            setLastUpdate(Date.now())
          }
        }
      }
    })

    return () => {
      unsubscribed = true
      unsubscribe()
    }
  }, [dataSources.length, enabled])

  // WebSocket events handling
  // Include telemetry and computed types to enable real-time updates
  const needsWebSocket = dataSources.some((ds) =>
    ds.type === 'websocket' ||
    ds.type === 'device' ||
    ds.type === 'metric' ||
    ds.type === 'command' ||
    ds.type === 'telemetry' ||
    ds.type === 'computed'
  )

  // Use device category instead of specific event types to receive all device events
  // This ensures we get events regardless of the exact type format (device.metric vs DeviceMetric)
  const { events } = useEvents({
    enabled: enabled && needsWebSocket,
    category: 'device',
    onConnected: (connected) => {
      if (!connected) {
        // Connection lost - clear event processing state
        processedEventsRef.current.clear()
        lastProcessedEventCountRef.current = 0
      }
    },
  })

  // Create a stable key from events that detects actual changes
  // Using last event ID + length ensures we detect new events even if total length stays same
  const eventsKey = useMemo(() => {
    if (events.length === 0) return 'empty'
    const lastEvent = events[events.length - 1]
    return `events-${events.length}-${lastEvent?.id || 'unknown'}`
  }, [events])

  // Process events - use events directly to ensure real-time updates
  // The eventsKey changes when new events arrive, triggering this effect reliably
  useEffect(() => {
    if (dataSources.length === 0 || !enabled) return

    if (events.length === 0) return

    // Only process new events since the last run
    // This prevents re-processing all events on every render
    const newEvents = events.slice(lastProcessedEventCountRef.current)
    if (newEvents.length === 0) return

    // Update the processed count
    lastProcessedEventCountRef.current = events.length

    // Process each new event
    for (const latestEvent of newEvents) {
      const eventData = (latestEvent as any).data || latestEvent
      const eventType = (latestEvent as any).type

      // Early exit: skip events that don't have a device_id we care about
      const hasDeviceId = eventData && typeof eventData === 'object' && 'device_id' in eventData
      if (hasDeviceId && relevantDeviceIds.size > 0) {
        const eventDeviceId = eventData.device_id as string
        if (!relevantDeviceIds.has(eventDeviceId)) {
          continue  // Skip events for devices we don't care about
        }
      }

      // Skip events we've already processed (by ID)
      const uniqueEventId = latestEvent.id || `${eventType}_${Date.now()}_${Math.random()}`
      if (processedEventsRef.current.has(uniqueEventId)) {
        continue
      }
      processedEventsRef.current.add(uniqueEventId)

      // Limit the processed events set size to prevent memory leaks
      // Use more aggressive cleanup to keep memory usage low
      if (processedEventsRef.current.size > 100) {
        const entries = Array.from(processedEventsRef.current)
        processedEventsRef.current = new Set(entries.slice(-50))
      }

      // Normalize event type - handle both PascalCase (DeviceMetric) and snake_case (device.metric)
      const normalizedEventType = eventType?.toLowerCase().replace('.', '')
      const isDeviceMetricEvent = normalizedEventType.includes('devicemetric') ||
                                  normalizedEventType.includes('metric') ||
                                  eventType === 'DeviceMetric'

      let shouldUpdate = false

      // Update store for device metric events: use metric name as property so
      // current_values[metricName] = value (e.g. current_values.temperature = 23.5)
      if (isDeviceMetricEvent && hasDeviceId) {
        const deviceId = eventData.device_id as string
        const store = useStore.getState()
        if ('metric' in eventData && 'value' in eventData) {
          store.updateDeviceMetric(deviceId, eventData.metric as string, eventData.value)
        }
        for (const [key, value] of Object.entries(eventData)) {
          if (key !== 'device_id' && key !== 'timestamp' && key !== 'type' && key !== 'id' && key !== 'metric' && key !== 'value') {
            store.updateDeviceMetric(deviceId, key, value)
          }
        }
        shouldUpdate = true
      }

      // Check if event matches data sources
      for (const ds of dataSources) {
        if (ds.type === 'device' && hasDeviceId && eventData.device_id === ds.deviceId && isDeviceMetricEvent) {
          shouldUpdate = true
          break
        } else if (ds.type === 'metric' && (isDeviceMetricEvent || eventType === 'metric.update')) {
          shouldUpdate = true
          break
        } else if (
          ds.type === 'command' &&
          hasDeviceId &&
          eventData.device_id === ds.deviceId &&
          (isDeviceMetricEvent || eventType === 'device.command_result')
        ) {
          shouldUpdate = true
          break
        } else if (
          // Telemetry sources: trigger refresh when matching device metric event occurs
          ds.type === 'telemetry' &&
          hasDeviceId &&
          eventData.device_id === ds.deviceId &&
          isDeviceMetricEvent
        ) {
          shouldUpdate = true
          break
        } else if (
          // Computed sources: trigger update when any relevant device data changes
          ds.type === 'computed' &&
          isDeviceMetricEvent
        ) {
          shouldUpdate = true
          break
        } else if (
          // Device-info sources: trigger update when device status changes (online/offline events)
          ds.type === 'device-info' &&
          hasDeviceId &&
          eventData.device_id === ds.deviceId &&
          (isDeviceMetricEvent || eventType === 'DeviceOnline' || eventType === 'DeviceOffline')
        ) {
          shouldUpdate = true
          break
        }
      }

      // For telemetry sources, merge event value directly and schedule cache refresh
      // This avoids race condition where old cached data is used during async API fetch
      const hasTelemetrySource = dataSources.some((ds) => ds.type === 'telemetry')
      if (hasTelemetrySource && isDeviceMetricEvent && hasDeviceId) {
        const eventDeviceId = eventData.device_id as string
        const matchingTelemetrySources = dataSources.filter((ds) =>
          ds.type === 'telemetry' && ds.deviceId === eventDeviceId
        )

        if (matchingTelemetrySources.length > 0) {
          const currentDataSources = dataSourcesRef.current
          const currentData = dataRef.current as unknown
          const now = Math.floor(Date.now() / 1000)

          // Use event timestamp if available, otherwise use current time
          // This ensures correct sorting when events are delayed
          // IMPORTANT: Event timestamps are in milliseconds, convert to seconds for consistency with telemetry data
          const rawEventTimestamp = (eventData as any).timestamp
          const eventTimestamp = rawEventTimestamp !== undefined
            ? (typeof rawEventTimestamp === 'number' && rawEventTimestamp > 10000000000
                ? Math.floor(rawEventTimestamp / 1000)  // Convert ms to seconds
                : rawEventTimestamp)  // Already in seconds or other format
            : now

          // Helper to get timestamp from point
          const getTs = (p: unknown): number => {
            if (p == null) return 0
            const o = p as Record<string, unknown>
            return (o.timestamp ?? o.time ?? o.t ?? 0) as number
          }

          // Merge event value into each matching telemetry source
          const updatedResults = currentDataSources.map((ds, index) => {
            if (ds.type !== 'telemetry' || ds.deviceId !== eventDeviceId) {
              return undefined
            }

            const metricId = ds.metricId || ds.property || 'value'
            let eventValue: unknown

            const eventMetric = typeof (eventData as any).metric === 'string' ? (eventData as any).metric : ''
            const hasValueKey = 'value' in eventData
            const metricMatches = eventMetric === metricId || eventMetricMatches(eventMetric, metricId)

            if (hasValueKey && metricMatches) {
              eventValue = eventData.value
            } else {
              eventValue = extractValueFromData(eventData, metricId)
            }

            if (eventValue === undefined) return undefined

            // Use event timestamp for correct temporal ordering
            const newPoint = { timestamp: eventTimestamp, time: eventTimestamp, value: eventValue }

            // Detect image data sources for special handling (no dedup, higher limit)
            const isImageDataSource = (ds.params?.includeRawPoints === true || ds.transform === 'raw') ||
                                   (metricId && (metricId.toLowerCase().includes('image') ||
                                                   metricId.toLowerCase().includes('img') ||
                                                   metricId.includes('values.image')))
            const maxLimit = isImageDataSource ? 200 : (ds.limit ?? 50)

            // Get current array for this source (handle multi-source case)
            const isPreserveMultiple = optionsRef.current.preserveMultiple
            let currentArray: unknown[] = []
            if (Array.isArray(currentData)) {
              if (isPreserveMultiple && currentDataSources.length > 1 && Array.isArray(currentData[index])) {
                currentArray = currentData[index] as unknown[]
              } else if (currentDataSources.length === 1 || !isPreserveMultiple) {
                currentArray = currentData as unknown[]
              }
            }

            // Merge new point and sort by timestamp (newest first)
            const merged = [newPoint, ...currentArray]
            const sorted = [...merged].sort((a, b) => getTs(b) - getTs(a))

            // For image data sources, skip deduplication to preserve all unique images
            if (isImageDataSource) {
              return sorted.slice(0, maxLimit)
            }
            return dedupeTelemetryPoints(sorted, getTs, maxLimit)
          })

          // Update data with merged values
          const hasUpdated = updatedResults.some((r) => r !== undefined)
          const validResults = updatedResults.filter((r) => r !== undefined)

          if (hasUpdated) {
            const { transform: transformFn } = optionsRef.current
            const isPreserveMultiple = optionsRef.current.preserveMultiple

            let finalData: unknown
            if (isPreserveMultiple && currentDataSources.length > 1) {
              finalData = updatedResults.map((r, i) => r ?? (Array.isArray(currentData) && Array.isArray(currentData[i]) ? currentData[i] : []))
            } else {
              finalData = updatedResults.find((r) => r !== undefined) ?? currentData
            }

            const transformedData = transformFn ? transformFn(finalData) : (finalData as T)
            setData(transformedData)
            setLastUpdate(Date.now())
          }

          // Schedule cache refresh after short delay to allow multiple rapid events to coalesce
          // This prevents excessive API calls while ensuring data freshness
          const refreshDelay = 2000 // 2 seconds delay
          matchingTelemetrySources.forEach((ds) => {
            const cacheKey = `${ds.deviceId}|${ds.metricId}|${ds.timeRange ?? 1}|${ds.limit ?? 50}|${ds.aggregate ?? ds.aggregateExt ?? 'raw'}`
            const cached = telemetryCache.get(cacheKey)

            // Only schedule refresh if cache exists and isn't being refreshed
            if (cached && !cached.refreshing) {
              telemetryCache.set(cacheKey, { ...cached, refreshing: true, refreshAfter: Date.now() + refreshDelay })
            }
          })

          // Use a single timeout to batch refreshes
          // Clear existing timer if any
          if (telemetryRefreshTimerRef.current) {
            clearTimeout(telemetryRefreshTimerRef.current)
          }
          telemetryRefreshTimerRef.current = setTimeout(() => {
            // Clear the refreshing flag and trigger actual fetch
            for (const [key, value] of telemetryCache.entries()) {
              if (value.refreshing && value.refreshAfter && Date.now() >= value.refreshAfter) {
                telemetryCache.delete(key)
              }
            }
            telemetryRefreshTimerRef.current = null
            setTelemetryRefreshTrigger(prev => prev + 1)
          }, refreshDelay)
        }
      }

      if (shouldUpdate) {
        const { transform: transformFn } = optionsRef.current

        // Extract value directly from event
        const currentDataSources = dataSourcesRef.current
        const currentDevices = useStore.getState().devices
        const currentData = data as any

        const results = currentDataSources.map((ds, index) => {
          let result: unknown

          switch (ds.type) {
            case 'static':
              result = ds.staticValue
              break

            case 'device': {
              const deviceId = ds.deviceId!
              const property = ds.property as string | undefined

              // If no property specified, return full device object
              if (!property) {
                result = currentDevices.find((d: Device) => d.id === deviceId || d.device_id === deviceId) ?? null
                break
              }

              if (isDeviceMetricEvent && eventData.device_id === deviceId) {
                const eventMetric = typeof eventData.metric === 'string' ? eventData.metric : ''
                if ('metric' in eventData && 'value' in eventData && (eventMetric === property || eventMetricMatches(eventMetric, property))) {
                  result = eventData.value
                  break
                }
                const extracted = extractValueFromData(eventData, property)
                if (extracted !== undefined) {
                  result = extracted
                  break
                }
              }

              const device = currentDevices.find((d: Device) => d.id === deviceId)
              if (device?.current_values && typeof device.current_values === 'object') {
                const extracted = extractValueFromData(device.current_values, property)
                result = extracted !== undefined ? extracted : '-'
              } else {
                result = '-'
              }
              result = safeExtractValue(result, '-')
              break
            }

            case 'metric': {
              const metricId = ds.metricId ?? 'value'

              if (isDeviceMetricEvent) {
                const eventMetric = typeof eventData.metric === 'string' ? eventData.metric : ''
                if ('metric' in eventData && 'value' in eventData && (eventMetric === metricId || eventMetricMatches(eventMetric, metricId))) {
                  result = eventData.value
                  break
                }
                const extracted = extractValueFromData(eventData, metricId)
                if (extracted !== undefined) {
                  result = extracted
                  break
                }
              }

              for (const device of currentDevices) {
                if (device.current_values && typeof device.current_values === 'object') {
                  const value = extractValueFromData(device.current_values, metricId)
                  if (value !== undefined) {
                    result = value
                    break
                  }
                }
              }

              if (result === undefined) {
                result = optionsRef.current.fallback ?? '-'
              }
              result = safeExtractValue(result, '-')
              break
            }

            case 'command': {
              const deviceId = ds.deviceId
              const property = ds.property || 'state'

              if (isDeviceMetricEvent && eventData.device_id === deviceId) {
                const eventMetric = typeof eventData.metric === 'string' ? eventData.metric : ''
                if ('metric' in eventData && 'value' in eventData && (eventMetric === property || eventMetricMatches(eventMetric, property))) {
                  result = eventData.value
                  break
                }
                const extracted = extractValueFromData(eventData, property)
                if (extracted !== undefined) {
                  result = extracted
                  break
                }
              }

              const device = currentDevices.find((d: Device) => d.id === deviceId)
              if (device?.current_values && typeof device.current_values === 'object') {
                const extracted = extractValueFromData(device.current_values, property)
                result = extracted !== undefined ? extracted : false
              } else {
                result = false
              }
              result = safeExtractValue(result, false)
              break
            }

            case 'device-info': {
              const deviceId = ds.deviceId
              const infoProperty = ds.infoProperty || 'name'
              const device = currentDevices.find((d: Device) => d.id === deviceId || d.device_id === deviceId)

              if (!device) {
                result = optionsRef.current.fallback ?? '-'
              } else {
                switch (infoProperty) {
                  case 'name':
                    result = device.name || '-'
                    break
                  case 'status':
                    result = device.status || 'unknown'
                    break
                  case 'online':
                    result = device.online ?? false
                    break
                  case 'last_seen':
                    result = device.last_seen || '-'
                    break
                  case 'device_type':
                    result = device.device_type || '-'
                    break
                  case 'plugin_name':
                    result = device.plugin_name || device.adapter_id || '-'
                    break
                  case 'adapter_id':
                    result = device.adapter_id || '-'
                    break
                  default:
                    result = optionsRef.current.fallback ?? '-'
                }
              }
              result = safeExtractValue(result as unknown, (optionsRef.current.fallback ?? '-') as any)
              break
            }

            case 'telemetry': {
              // When event matches this telemetry source, merge event value into data immediately
              // so image/history components update without waiting for fetch or device in store.
              if (isDeviceMetricEvent && hasDeviceId && eventData.device_id === ds.deviceId) {
                const metricId = ds.metricId || ds.property || 'value'
                let eventValue: unknown
                const eventMetric = typeof (eventData as any).metric === 'string' ? (eventData as any).metric : ''
                if ('value' in eventData && (eventMetric === metricId || eventMetricMatches(eventMetric, metricId))) {
                  eventValue = eventData.value
                } else {
                  eventValue = extractValueFromData(eventData, metricId)
                }
                if (eventValue !== undefined) {
                  // Use event timestamp for correct temporal ordering instead of current time
                  // Event timestamps are in milliseconds, convert to seconds for consistency
                  const rawEventTimestamp = (eventData as any).timestamp
                  const eventTimestamp = rawEventTimestamp !== undefined
                    ? (typeof rawEventTimestamp === 'number' && rawEventTimestamp > 10000000000
                        ? Math.floor(rawEventTimestamp / 1000)  // Convert ms to seconds
                        : rawEventTimestamp)  // Already in seconds
                    : Math.floor(Date.now() / 1000)  // Fallback to current time
                  const newPoint = { timestamp: eventTimestamp, time: eventTimestamp, value: eventValue }
                  const currentArray = currentDataSources.length > 1 && Array.isArray(currentData) && currentData[index] !== undefined
                    ? (Array.isArray(currentData[index]) ? (currentData[index] as unknown[]) : [])
                    : (Array.isArray(currentData) ? (currentData as unknown[]) : [])
                  const merged = [newPoint, ...currentArray]

                  // Detect image data sources for special handling (no dedup, higher limit)
                  const isImageDataSource = (ds.params?.includeRawPoints === true || ds.transform === 'raw') ||
                                           (metricId && (metricId.toLowerCase().includes('image') ||
                                                          metricId.toLowerCase().includes('img') ||
                                                          metricId.includes('values.image')))
                  const maxLimit = isImageDataSource ? 200 : (ds.limit ?? 50)

                  const getTs = (p: unknown): number => {
                    if (p === null || p === undefined) return 0
                    const o = p as Record<string, unknown>
                    return (o.timestamp ?? o.time ?? o.t ?? 0) as number
                  }
                  const sorted = [...merged].sort((a, b) => getTs(b) - getTs(a))

                  // For image data sources, skip deduplication to preserve all unique images
                  if (isImageDataSource) {
                    result = sorted.slice(0, maxLimit)
                  } else {
                    result = dedupeTelemetryPoints(sorted, getTs, maxLimit)
                  }
                  break
                }
              }
              // Fallback: preserve current value for this slot
              if (currentDataSources.length > 1 && Array.isArray(currentData) && currentData[index] !== undefined) {
                result = currentData[index]
              } else if (Array.isArray(currentData) && currentData.length > 0) {
                result = currentData
              } else {
                result = optionsRef.current.fallback ?? []
              }
              break
            }

            default:
              return
          }

          return result
        })

        let finalData: unknown
        if (currentDataSources.length > 1) {
          finalData = results
        } else {
          finalData = results[0]
        }

        const transformedData = transformFn ? transformFn(finalData) : (finalData as T)
        setData(transformedData)
        setLastUpdate(Date.now())
      }
    }
  }, [enabled, dataSourceKey, eventsKey])  // eventsKey ensures effect runs when new events arrive

  // Telemetry data fetching (for historical time-series data)
  // Use stable key for dependency to prevent infinite re-renders
  const telemetryKey = useMemo(() => {
    return dataSources
      .filter((ds) => ds.type === 'telemetry')
      .map((ds) => {
        // Use actual values that will be used for fetching, to ensure key matches cache
        const isImageDataSource = (ds.params?.includeRawPoints === true || ds.transform === 'raw') ||
                                 (ds.metricId && (ds.metricId.toLowerCase().includes('image') ||
                                                   ds.metricId.toLowerCase().includes('img') ||
                                                   ds.metricId.includes('values.image')))
        const actualTimeRange = ds.timeRange && ds.timeRange > 1 ? ds.timeRange : (isImageDataSource ? 48 : 1)
        const actualLimit = isImageDataSource ? 200 : (ds.limit ?? 50)
        const actualAggregate = ds.aggregate ?? ds.aggregateExt ?? 'raw'

        return createStableKey({
          deviceId: ds.deviceId,
          metricId: ds.metricId,
          timeRange: actualTimeRange,
          limit: actualLimit,
          aggregate: actualAggregate
        })
      })
      .join('|')
  }, [dataSources])

  const telemetryDataSources = useMemo(() => {
    return dataSources.filter((ds) => ds.type === 'telemetry')
  }, [dataSources])

  const hasTelemetrySource = telemetryDataSources.length > 0

  useEffect(() => {
    if (!hasTelemetrySource || !enabled) {
      // Clean up any existing interval when disabled
      if (intervalRef.current) {
        clearInterval(intervalRef.current)
        intervalRef.current = null
      }
      return
    }

    // Detect telemetry config changes and reset loading state
    const configChanged = prevTelemetryKeyRef.current !== telemetryKey
    if (configChanged && telemetryKey) {
      initialTelemetryFetchDoneRef.current = false
    }
    prevTelemetryKeyRef.current = telemetryKey

    const fetchTelemetryData = async () => {
      // Only show loading state on initial fetch, not on interval refreshes
      // Use ref to persist state across effect re-runs
      const isInitialFetch = !initialTelemetryFetchDoneRef.current
      if (isInitialFetch) {
        setLoading(true)
      }
      setError(null)

      // Add timeout protection to prevent infinite loading
      const timeoutPromise = new Promise((_, reject) =>
        setTimeout(() => reject(new Error('Fetch timeout')), 10000)
      )

      try {
        // Race between fetch and timeout to prevent hanging
        const results = await Promise.race([
          Promise.all(
            telemetryDataSources.map(async (ds) => {
            if (!ds.deviceId || !ds.metricId) {
              return { data: [], raw: undefined }
            }

            // Check if raw points are needed (for image history, etc.)
            const includeRawPoints = ds.params?.includeRawPoints === true || ds.transform === 'raw'

            // Bypass cache on initial fetch, and always bypass for raw/image history so we don't
            // mix one fresh point with stale cached points (which can show "first latest, rest hours ago")
            const bypassCache = !initialTelemetryFetchDoneRef.current || includeRawPoints

            // For image data (raw points with base64 content), use higher limits and longer time range
            const isImageDataSource = includeRawPoints ||
                                    (ds.metricId && (ds.metricId.toLowerCase().includes('image') ||
                                                      ds.metricId.toLowerCase().includes('img') ||
                                                      ds.metricId.includes('values.image')))

            // Use the data source's timeRange if set, otherwise use appropriate defaults
            // Image data sources may have larger timeRanges configured by the component
            const actualTimeRange = ds.timeRange && ds.timeRange > 1 ? ds.timeRange : (isImageDataSource ? 48 : 1)
            const actualLimit = isImageDataSource ? 200 : (ds.limit ?? 50)
            const actualAggregate = ds.aggregate ?? ds.aggregateExt ?? 'raw'

            const response = await fetchHistoricalTelemetry(
              ds.deviceId,
              ds.metricId,
              actualTimeRange,
              actualLimit,
              actualAggregate,
              includeRawPoints,
              bypassCache
            )

            // Return raw data if requested, otherwise return values
            if (includeRawPoints && response.raw) {
              return { data: response.data, raw: response.raw, success: response.success }
            }
            return { data: response.success ? response.data : [], success: response.success }
          })
          ),
          timeoutPromise
        ]) as Array<{ data: unknown[]; raw?: unknown[]; success: boolean }>

        // Combine results
        let finalData: unknown

        if (results.length > 1) {
          // If preserveMultiple is true, keep each source's data separate
          if (optionsRef.current.preserveMultiple) {
            // Return array of data arrays, one per source
            const hasRawData = results.some((r: { data: unknown[]; raw?: unknown[]; success: boolean }) => r.raw !== undefined)
            if (hasRawData) {
              finalData = results.map((r: { data: unknown[]; raw?: unknown[]; success: boolean }) => r.raw ?? [])
            } else {
              finalData = results.map((r: { data: unknown[]; raw?: unknown[]; success: boolean }) => r.data ?? [])
            }
          } else {
            // Original behavior: merge all data
            const hasRawData = results.some((r: { data: unknown[]; raw?: unknown[]; success: boolean }) => r.raw !== undefined)
            if (hasRawData) {
              // Combine raw data from all sources
              const allRawData = results.flatMap((r: { data: unknown[]; raw?: unknown[]; success: boolean }) => r.raw ?? [])
              finalData = allRawData
            } else {
              finalData = results.map((r: { data: unknown[]; raw?: unknown[]; success: boolean }) => r.data ?? []).flat()
            }
          }
        } else {
          const singleResult = results[0] as { data: unknown[]; raw?: unknown[]; success: boolean } | undefined
          finalData = (singleResult?.raw ?? singleResult?.data) ?? []
        }

        // CRITICAL: Always merge with latest values from store for real-time updates
        // This ensures components see the latest data even if API hasn't persisted it yet
        const storeState = useStore.getState()

        // Helper to check if a value looks like an image (base64 or data URL)
        const looksLikeImage = (val: unknown): boolean => {
          if (typeof val !== 'string') return false
          const str = val.trim()
          return str.startsWith('data:image/') ||
                 str.startsWith('data:base64,') ||
                 (str.length > 100 && /^[A-Za-z0-9+/=_-]+$/.test(str))
        }

        // Helper to find metric value with fuzzy matching
        const findMetricValue = (
          currentValues: Record<string, unknown>,
          metricId: string
        ): { value: unknown; matchedKey: string } | undefined => {
          // 1. Try exact match
          if (metricId in currentValues) {
            return { value: currentValues[metricId], matchedKey: metricId }
          }

          // 2. Try case-insensitive match
          const lowerMetricId = metricId.toLowerCase()
          for (const key of Object.keys(currentValues)) {
            if (key.toLowerCase() === lowerMetricId) {
              return { value: currentValues[key], matchedKey: key }
            }
          }

          // 3. Try with common suffixes/prefixes removed
          const baseName = metricId
            .replace(/_base64$/i, '')
            .replace(/^image_/i, '')
            .replace(/^img_/i, '')
            .replace(/_url$/i, '')
            .replace(/_str$/i, '')
          for (const key of Object.keys(currentValues)) {
            const keyBase = key
              .replace(/_base64$/i, '')
              .replace(/^image_/i, '')
              .replace(/^img_/i, '')
              .replace(/_url$/i, '')
              .replace(/_str$/i, '')
            if (keyBase.toLowerCase() === baseName.toLowerCase()) {
              return { value: currentValues[key], matchedKey: key }
            }
          }

          // 4. For image-like metrics, try to find any value that looks like an image
          if (metricId.toLowerCase().includes('image') || metricId.toLowerCase().includes('img')) {
            for (const key of Object.keys(currentValues)) {
              if (looksLikeImage(currentValues[key])) {
                return { value: currentValues[key], matchedKey: key }
              }
            }
          }

          // 5. Try nested path like "values.image"
          const parts = metricId.split('.')
          let nested: unknown = currentValues
          for (const part of parts) {
            if (nested && typeof nested === 'object' && part in nested) {
              nested = (nested as Record<string, unknown>)[part]
            } else {
              nested = undefined
              break
            }
          }
          if (nested !== undefined) {
            return { value: nested, matchedKey: metricId }
          }

          return undefined
        }

        const telemetryDataSourcesWithStore = telemetryDataSources.map((ds) => {
          const device = storeState.devices.find((d: Device) => d.id === ds.deviceId || d.device_id === ds.deviceId)
          if (!device?.current_values) return { dataSource: ds, latestValue: undefined }

          // Get the latest value for this metric from store with fuzzy matching
          const metricId = ds.metricId || ds.property || 'value'
          const matchResult = findMetricValue(device.current_values as Record<string, unknown>, metricId)
          const latestValue = matchResult?.value

          return { dataSource: ds, latestValue, deviceId: ds.deviceId, metricId, matchedKey: matchResult?.matchedKey }
        })

        // CRITICAL FIX: Add store value if it exists
        // This ensures real-time updates from WebSocket events are shown (e.g. image components)
        const hasStoreValues = telemetryDataSourcesWithStore.some((item) => item.latestValue !== undefined)
        const now = Math.floor(Date.now() / 1000)

        const valuesEqual = (a: unknown, b: unknown): boolean => {
          if (a === b) return true
          if (a === null || a === undefined || b === null || b === undefined) return false
          if (typeof a === 'object' && typeof b === 'object') {
            try {
              return JSON.stringify(a) === JSON.stringify(b)
            } catch {
              return false
            }
          }
          return false
        }

        if (hasStoreValues) {
          // Start from API data if we have it, otherwise from empty (so image/components get store-only data)
          let rawDataArray: unknown[] = Array.isArray(finalData) ? [...finalData] : []

          for (const storeItem of telemetryDataSourcesWithStore) {
            if (storeItem.latestValue === undefined) continue

            const latestValue = storeItem.latestValue
            const maxLimit = telemetryDataSources[0].limit ?? 50

            // Add store value when: no API data, or value differs from first point, or first point is old (>30s)
            const firstPoint = rawDataArray[0] as { timestamp?: number; time?: number; value?: unknown } | undefined
            const firstValue = firstPoint?.value
            const firstTimestamp = (firstPoint?.timestamp ?? firstPoint?.time ?? 0) as number
            const firstPointAge = now - firstTimestamp

            const shouldAddNewPoint = rawDataArray.length === 0 ||
                                      !valuesEqual(firstValue, latestValue) ||
                                      firstPointAge > 30

            if (shouldAddNewPoint) {
              const newPoint = {
                timestamp: now,
                time: now,
                value: latestValue,
              }
              rawDataArray.unshift(newPoint)
            }
          }

          finalData = rawDataArray
        }

        // For telemetry (e.g. image history): sort by latest time, dedupe (incl. near-duplicates), then take first N
        if (Array.isArray(finalData) && telemetryDataSources.length > 0) {
          const ds = telemetryDataSources[0]
          // Detect image data sources and use higher limits
          const isImageDataSource = (ds.params?.includeRawPoints === true || ds.transform === 'raw') ||
                                   (ds.metricId && (ds.metricId.toLowerCase().includes('image') ||
                                                     ds.metricId.toLowerCase().includes('img') ||
                                                     ds.metricId.includes('values.image')))

          const maxLimit = isImageDataSource ? 200 : (ds.limit ?? 50)
          const getTs = (p: unknown): number => {
            if (p === null || p === undefined) return 0
            const o = p as Record<string, unknown>
            return (o.timestamp ?? o.time ?? o.t ?? 0) as number
          }
          const sorted = [...finalData].sort((a, b) => getTs(b) - getTs(a))

          // For image data, skip deduplication entirely to preserve all unique images
          if (isImageDataSource) {
            // For images, just take first N without deduplication
            finalData = sorted.slice(0, maxLimit)
          } else {
            const deduped = dedupeTelemetryPoints(sorted, getTs, maxLimit)
            finalData = deduped
          }
        }

        const { transform: transformFn } = optionsRef.current
        const transformedData = transformFn ? transformFn(finalData) : (finalData as T)

        setData(transformedData)
        setLastUpdate(Date.now())
        initialTelemetryFetchDoneRef.current = true
      } catch (err) {
        logError(err, { operation: 'Fetch telemetry data' })
        const errorMessage = err instanceof Error ? err.message : 'Failed to fetch telemetry'
        setError(errorMessage)
        const fallbackData = optionsRef.current.fallback ?? []
        setData(fallbackData as T)
        initialTelemetryFetchDoneRef.current = true
      } finally {
        // Always set loading to false, even if there's an error
        setLoading(false)
      }
    }

    // Clean up existing interval before creating a new one
    if (intervalRef.current) {
      clearInterval(intervalRef.current)
      intervalRef.current = null
    }

    fetchTelemetryData()

    // Set up refresh interval if specified (refresh is in seconds, convert to ms)
    const refreshIntervals = telemetryDataSources.map((ds) => ds.refresh).filter(Boolean) as number[]
    const minRefreshSeconds = refreshIntervals.length > 0 ? Math.min(...refreshIntervals) : null

    if (minRefreshSeconds) {
      const minRefreshMs = minRefreshSeconds * 1000
      intervalRef.current = setInterval(fetchTelemetryData, minRefreshMs)
    }

    // Cleanup function - always clear interval on unmount or dependency change
    return () => {
      if (intervalRef.current) {
        clearInterval(intervalRef.current)
        intervalRef.current = null
      }
    }
  }, [telemetryKey, enabled, telemetryRefreshTrigger])

  // System data fetching (single values from system stats)
  const systemKey = useMemo(() => {
    return dataSources
      .filter((ds) => ds.type === 'system')
      .map((ds) => createStableKey({
        systemMetric: ds.systemMetric,
      }))
      .join('|')
  }, [dataSources])

  const systemDataSources = useMemo(() => {
    return dataSources.filter((ds) => ds.type === 'system')
  }, [dataSources])

  const hasSystemSource = systemDataSources.length > 0

  useEffect(() => {
    if (!hasSystemSource || !enabled) {
      // Clean up any existing interval when disabled
      if (intervalRef.current) {
        clearInterval(intervalRef.current)
        intervalRef.current = null
      }
      return
    }

    const fetchSystemData = async () => {
      // Only show loading state on initial fetch, not on interval refreshes
      if (!initialSystemFetchDoneRef.current) {
        setLoading(true)
      }
      setError(null)

      try {
        const results = await Promise.all(
          systemDataSources.map(async (ds) => {
            const metric = ds.systemMetric
            if (!metric) {
              return { data: null }
            }

            const response = await fetchSystemStats(metric)
            return { data: response.data, success: response.success }
          })
        )

        // Combine results
        let finalData: unknown
        if (results.length > 1) {
          finalData = results.map((r) => r.data)
        } else {
          finalData = results[0]?.data ?? null
        }

        const { transform: transformFn } = optionsRef.current
        const transformedData = transformFn ? transformFn(finalData) : (finalData as T)
        setData(transformedData)
        setLastUpdate(Date.now())
        initialSystemFetchDoneRef.current = true
      } catch (err) {
        logError(err, { operation: 'Fetch system data' })
        const errorMessage = err instanceof Error ? err.message : 'Failed to fetch system data'
        setError(errorMessage)
        const fallbackData = optionsRef.current.fallback ?? null
        setData(fallbackData as T)
        initialSystemFetchDoneRef.current = true
      } finally {
        setLoading(false)
      }
    }

    // Clean up existing interval before creating a new one
    if (intervalRef.current) {
      clearInterval(intervalRef.current)
      intervalRef.current = null
    }

    fetchSystemData()

    // Set up refresh interval if specified (refresh is in seconds, convert to ms)
    const refreshIntervals = systemDataSources.map((ds) => ds.refresh).filter(Boolean) as number[]
    const minRefreshSeconds = refreshIntervals.length > 0 ? Math.min(...refreshIntervals) : null

    if (minRefreshSeconds) {
      const minRefreshMs = minRefreshSeconds * 1000
      intervalRef.current = setInterval(fetchSystemData, minRefreshMs)
    }

    // Cleanup function - always clear interval on unmount or dependency change
    return () => {
      if (intervalRef.current) {
        clearInterval(intervalRef.current)
        intervalRef.current = null
      }
    }
  }, [systemKey, enabled])

  // Cleanup effect for telemetry refresh timer
  useEffect(() => {
    return () => {
      if (telemetryRefreshTimerRef.current) {
        clearTimeout(telemetryRefreshTimerRef.current)
        telemetryRefreshTimerRef.current = null
      }
    }
  }, [])

  return {
    data,
    loading,
    error,
    lastUpdate,
    ...(hasCommandSource && { sendCommand, sending }),
  }
}

// ============================================================================
// Specialized Hooks
// ============================================================================

/**
 * Hook for number array data sources
 */
export function useNumberArrayDataSource(
  dataSource: DataSourceOrList | undefined,
  options?: {
    enabled?: boolean
    fallback?: number[]
  }
) {
  const { data, loading, error, lastUpdate } = useDataSource<number[]>(dataSource, {
    ...options,
    transform: (raw): number[] => toNumberArray(raw, options?.fallback ?? []),
    fallback: options?.fallback ?? [],
  })

  return {
    data: data ?? [],
    loading,
    error,
    lastUpdate,
  }
}
