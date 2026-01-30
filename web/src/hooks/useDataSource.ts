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
import { useEvents } from '@/hooks/useEvents'
import { useStore } from '@/store'
import { toNumberArray, isEmpty, isValidNumber } from '@/design-system/utils/format'

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
const telemetryCache = new Map<string, { data: number[]; raw?: any[]; timestamp: number }>()
const TELEMETRY_CACHE_TTL = 5000 // 5 seconds cache

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
): Promise<{ data: number[]; raw?: any[]; success: boolean }> {
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
    const response = await api.getDeviceTelemetry(deviceId, metricId, Math.floor(start / 1000), Math.floor(now / 1000), fetchLimit)

    // TelemetryDataResponse has structure: { data: Record<string, TelemetryPoint[]> }
    // The actual time series data is in response.data[metricId]
    if (response?.data && typeof response.data === 'object') {
      const metricData = response.data[metricId]

      if (Array.isArray(metricData) && metricData.length > 0) {
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

        // Helper to extract timestamp from a point
        const extractTimestamp = (point: unknown): number => {
          if (typeof point === 'object' && point !== null) {
            const p = point as unknown as Record<string, unknown>
            const timestamp = p.timestamp ?? p.time ?? p.t
            if (typeof timestamp === 'number') return timestamp
          }
          return Date.now() / 1000
        }

        // Extract all values
        const allValues = metricData.map(extractValue).filter((v: number) => typeof v === 'number' && !isNaN(v))

        // Apply aggregation
        // NOTE: API returns data in DESCENDING order (newest first: index 0 = newest)
        let values: number[]
        let rawPoints: any[] | undefined

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
          rawPoints = includeRawPoints ? metricData.map((point) => {
            if (typeof point === 'number') {
              return { timestamp: Date.now() / 1000, value: point }
            }
            if (typeof point === 'object' && point !== null) {
              const p = point as unknown as Record<string, unknown>
              const timestamp = p.timestamp ?? p.time ?? p.t ?? Date.now() / 1000
              const value = p.value ?? p.v ?? 0
              return { timestamp: timestamp as number, value }
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
    }

    return { data: [], success: false }
  } catch (error) {
    console.error('[fetchHistoricalTelemetry] Error:', error)
    return { data: [], success: false }
  }
}

/**
 * Clear expired telemetry cache entries
 */
function cleanupTelemetryCache() {
  const now = Date.now()
  for (const [key, value] of telemetryCache.entries()) {
    if (now - value.timestamp > TELEMETRY_CACHE_TTL) {
      telemetryCache.delete(key)
    }
  }
}

// Periodic cache cleanup
if (typeof window !== 'undefined') {
  setInterval(cleanupTelemetryCache, 60000) // Clean up every minute
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
 * Extract value from nested object using dot notation
 */
function extractValueFromData(data: unknown, property: string): unknown {
  if (data === null || data === undefined) return undefined
  if (typeof data !== 'object') return data

  const dataObj = data as Record<string, unknown>

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

  // Direct access
  if (property in dataObj) return dataObj[property]

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
 * Helper function to create stable JSON key for memoization
 * Handles objects with potentially different property order
 */
function createStableKey(obj: any): string {
  if (obj === null || obj === undefined) return ''
  if (typeof obj !== 'object') return String(obj)
  if (Array.isArray(obj)) return '[' + obj.map(createStableKey).join(',') + ']'
  const sortedKeys = Object.keys(obj).sort()
  return '{' + sortedKeys.map(k => `"${k}":${createStableKey(obj[k])}`).join(',') + '}'
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

  // CRITICAL: Memoize dataSources to prevent infinite re-renders
  // Using stable key generation ensures consistency
  const dataSourceKey = useMemo(() => {
    return createStableKey(dataSource)
  }, [dataSource])

  const dataSources = useMemo(() => {
    return dataSource ? normalizeDataSource(dataSource) : []
  }, [dataSourceKey])

  const initialFetchDoneRef = useRef<Set<string>>(new Set())
  const lastValidDataRef = useRef<Record<string, unknown>>({})

  const optionsRef = useRef({ enabled, transform, fallback, preserveMultiple })
  optionsRef.current = { enabled, transform, fallback, preserveMultiple }

  const dataSourcesRef = useRef(dataSources)
  dataSourcesRef.current = dataSources

  // Ref to track if initial telemetry fetch has completed
  // This prevents showing loading state on refreshes/updates
  const initialTelemetryFetchDoneRef = useRef(false)

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
      // Filter out telemetry sources - they are handled separately by the fetch effect
      const nonTelemetrySources = currentDataSources.filter((ds) => ds.type !== 'telemetry')

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
            const device = currentDevices.find((d: any) => d.id === deviceId || d.device_id === deviceId)

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
            const device = currentDevices.find((d: any) => d.id === deviceId)

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
            const device = currentDevices.find((d: any) => d.id === deviceId || d.device_id === deviceId)

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
                // eslint-disable-next-line no-new-func
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
        // All sources are telemetry - the telemetry effect will handle this
        // For single telemetry sources, telemetry effect sets the data
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

    readDataFromStore()

    const unsubscribe = useStore.subscribe((state: any, prevState: any) => {
      const devicesChanged = state.devices.length !== prevState.devices.length
      let currentValuesChanged = false

      if (!devicesChanged) {
        const currentDataSources = dataSourcesRef.current
        const sourceDeviceIds = new Set(
          currentDataSources
            .map((ds) => ds.type === 'device' || ds.type === 'command' ? ds.deviceId : null)
            .filter(Boolean) as string[]
        )

        for (const deviceId of sourceDeviceIds) {
          const device = state.devices.find((d: any) => d.id === deviceId || d.device_id === deviceId)
          const prevDevice = prevState.devices.find((d: any) => d.id === deviceId || d.device_id === deviceId)

          if (device && prevDevice) {
            const currentJson = JSON.stringify(device.current_values)
            const prevJson = JSON.stringify(prevDevice.current_values)
            if (currentJson !== prevJson) {
              const hasDataNow = device.current_values && Object.keys(device.current_values).length > 0
              if (hasDataNow) {
                currentValuesChanged = true
                break
              }
            }
          } else if (device && !prevDevice) {
            if (device.current_values && Object.keys(device.current_values).length > 0) {
              currentValuesChanged = true
              break
            }
          }
        }
      }

      if (devicesChanged || currentValuesChanged) {
        readDataFromStore()
      }
    })

    return unsubscribe
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
  })

  useEffect(() => {
    if (dataSources.length === 0 || !enabled || events.length === 0) return

    const latestEvent = events[events.length - 1]
    if (!latestEvent) return

    const eventData = (latestEvent as any).data || latestEvent
    const eventType = (latestEvent as any).type

    // Normalize event type - handle both PascalCase (DeviceMetric) and snake_case (device.metric)
    const normalizedEventType = eventType?.toLowerCase().replace('.', '')
    const isDeviceMetricEvent = normalizedEventType.includes('devicemetric') ||
                                normalizedEventType.includes('metric') ||
                                eventType === 'DeviceMetric'
    const hasDeviceId = eventData && typeof eventData === 'object' && 'device_id' in eventData

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
      }
    }

    // For telemetry sources, also trigger telemetry cache invalidation and refetch
    const hasTelemetrySource = dataSources.some((ds) => ds.type === 'telemetry')
    if (hasTelemetrySource && isDeviceMetricEvent && hasDeviceId) {
      const eventDeviceId = eventData.device_id as string
      const matchingTelemetrySources = dataSources.filter((ds) =>
        ds.type === 'telemetry' && ds.deviceId === eventDeviceId
      )

      // Invalidate telemetry cache for affected sources and trigger refresh
      if (matchingTelemetrySources.length > 0) {
        matchingTelemetrySources.forEach((ds) => {
          const cacheKey = `${ds.deviceId}|${ds.metricId}|${ds.timeRange ?? 1}|${ds.limit ?? 50}|${ds.aggregate ?? ds.aggregateExt ?? 'raw'}`
          telemetryCache.delete(cacheKey)
        })

        // Trigger telemetry refresh by updating the trigger state
        setTelemetryRefreshTrigger(prev => prev + 1)
      }
    }

    if (shouldUpdate) {
      const { transform: transformFn } = optionsRef.current

      // Extract value directly from event
      const currentDataSources = dataSourcesRef.current
      const currentDevices = useStore.getState().devices

      const results = currentDataSources.map((ds) => {
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
              result = currentDevices.find((d: any) => d.id === deviceId || d.device_id === deviceId) ?? null
              break
            }

            if (isDeviceMetricEvent && eventData.device_id === deviceId) {
              if ('metric' in eventData && eventData.metric === property && 'value' in eventData) {
                result = eventData.value
                break
              }
              const extracted = extractValueFromData(eventData, property)
              if (extracted !== undefined) {
                result = extracted
                break
              }
            }

            const device = currentDevices.find((d: any) => d.id === deviceId)
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
              if ('metric' in eventData && eventData.metric === metricId && 'value' in eventData) {
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
              if ('metric' in eventData && eventData.metric === property && 'value' in eventData) {
                result = eventData.value
                break
              }
              const extracted = extractValueFromData(eventData, property)
              if (extracted !== undefined) {
                result = extracted
                break
              }
            }

            const device = currentDevices.find((d: any) => d.id === deviceId)
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
            const device = currentDevices.find((d: any) => d.id === deviceId || d.device_id === deviceId)

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
            // For telemetry sources, try to get the latest value from store on WebSocket events
            // This provides immediate updates while the API fetch runs in background
            const deviceId = ds.deviceId
            const metricId = ds.metricId || ds.property || 'value'

            if (isDeviceMetricEvent && eventData.device_id === deviceId) {
              // Try to extract value from the event first
              if ('metric' in eventData && eventData.metric === metricId && 'value' in eventData) {
                result = eventData.value
                break
              }
              const extracted = extractValueFromData(eventData, metricId)
              if (extracted !== undefined) {
                result = extracted
                break
              }
            }

            // If not found in event, try to get from store (latest device state)
            const device = currentDevices.find((d: any) => d.id === deviceId || d.device_id === deviceId)
            if (device?.current_values && typeof device.current_values === 'object') {
              const extracted = extractValueFromData(device.current_values, metricId)
              if (extracted !== undefined) {
                result = extracted
                break
              }
            }

            // If no latest value found, preserve current data for display
            // This prevents flickering while waiting for API response
            const currentData = data as any
            if (result === undefined) {
              if (Array.isArray(currentData) && currentData.length > 0) {
                // Keep existing data, will be updated by telemetry fetch
                result = currentData
              } else {
                result = optionsRef.current.fallback ?? '-'
              }
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
  }, [events, dataSources, enabled])

  // Telemetry data fetching (for historical time-series data)
  // Use stable key for dependency to prevent infinite re-renders
  const telemetryKey = useMemo(() => {
    return dataSources
      .filter((ds) => ds.type === 'telemetry')
      .map((ds) => createStableKey({
        deviceId: ds.deviceId,
        metricId: ds.metricId,
        timeRange: ds.timeRange,
        limit: ds.limit,
        aggregate: ds.aggregate ?? ds.aggregateExt
      }))
      .join('|')
  }, [dataSources])

  const telemetryDataSources = useMemo(() => {
    return dataSources.filter((ds) => ds.type === 'telemetry')
  }, [dataSources])

  const hasTelemetrySource = telemetryDataSources.length > 0

  useEffect(() => {
    if (!hasTelemetrySource || !enabled) {
      return
    }

    const fetchTelemetryData = async () => {
      // Only show loading state on initial fetch, not on interval refreshes
      // Use ref to persist state across effect re-runs
      if (!initialTelemetryFetchDoneRef.current) {
        setLoading(true)
      }
      setError(null)

      try {
        const results = await Promise.all(
          telemetryDataSources.map(async (ds) => {
            if (!ds.deviceId || !ds.metricId) {
              return { data: [], raw: undefined }
            }

            // Check if raw points are needed (for image history, etc.)
            const includeRawPoints = ds.params?.includeRawPoints === true || ds.transform === 'raw'

            // Bypass cache on initial fetch to ensure we get the latest data
            // This is especially important for image components that need the most recent value
            const bypassCache = !initialTelemetryFetchDoneRef.current

            const response = await fetchHistoricalTelemetry(
              ds.deviceId,
              ds.metricId,
              ds.timeRange ?? 1,
              ds.limit ?? 50,
              ds.aggregate ?? ds.aggregateExt ?? 'raw',
              includeRawPoints,
              bypassCache
            )

            // Return raw data if requested, otherwise return values
            if (includeRawPoints && response.raw) {
              return { data: response.data, raw: response.raw, success: response.success }
            }
            return { data: response.success ? response.data : [], success: response.success }
          })
        )

        // Combine results
        let finalData: unknown
        if (results.length > 1) {
          // If preserveMultiple is true, keep each source's data separate
          if (optionsRef.current.preserveMultiple) {
            // Return array of data arrays, one per source
            const hasRawData = results.some((r: any) => r.raw)
            if (hasRawData) {
              finalData = results.map((r: any) => r.raw ?? [])
            } else {
              finalData = results.map((r: any) => r.data ?? [])
            }
          } else {
            // Original behavior: merge all data
            const hasRawData = results.some((r: any) => r.raw)
            if (hasRawData) {
              // Combine raw data from all sources
              const allRawData = results.flatMap((r: any) => r.raw ?? [])
              finalData = allRawData
            } else {
              finalData = results.map((r: any) => r.data ?? []).flat()
            }
          }
        } else {
          const singleResult = results[0] as any
          finalData = singleResult.raw ?? singleResult.data ?? []
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
          let nested: any = currentValues
          for (const part of parts) {
            if (nested && typeof nested === 'object' && part in nested) {
              nested = nested[part]
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
          const device = storeState.devices.find((d: any) => d.id === ds.deviceId || d.device_id === ds.deviceId)
          if (!device?.current_values) return { dataSource: ds, latestValue: undefined }

          // Get the latest value for this metric from store with fuzzy matching
          const metricId = ds.metricId || ds.property || 'value'
          const matchResult = findMetricValue(device.current_values as Record<string, unknown>, metricId)
          const latestValue = matchResult?.value

          return { dataSource: ds, latestValue, deviceId: ds.deviceId, metricId, matchedKey: matchResult?.matchedKey }
        })

        // CRITICAL FIX: ALWAYS add store value if it exists, even if API data is newer
        // This ensures the latest real-time value is always shown
        const hasStoreValues = telemetryDataSourcesWithStore.some((item) => item.latestValue !== undefined)
        if (hasStoreValues) {
          // Ensure finalData is an array
          let rawDataArray = Array.isArray(finalData) ? [...finalData] : []
          const now = Math.floor(Date.now() / 1000)

          for (const storeItem of telemetryDataSourcesWithStore) {
            if (storeItem.latestValue === undefined) continue

            const latestValue = storeItem.latestValue
            const maxTime = now
            const maxLimit = telemetryDataSources[0].limit ?? 100

            // Create a new point with current timestamp
            const newPoint = {
              timestamp: maxTime,  // Current time - ensures this is the "latest"
              time: maxTime,       // Also set 'time' for compatibility
              value: latestValue,
            }

            // ALWAYS add to beginning, even if it might be duplicate
            // The component will pick the one with the latest timestamp
            rawDataArray.unshift(newPoint)

            // Trim to limit
            if (rawDataArray.length > maxLimit) {
              rawDataArray = rawDataArray.slice(0, maxLimit)
            }

            // Store value merged successfully
          }

          finalData = rawDataArray
        }

        const { transform: transformFn } = optionsRef.current
        const transformedData = transformFn ? transformFn(finalData) : (finalData as T)
        setData(transformedData)
        setLastUpdate(Date.now())
        initialTelemetryFetchDoneRef.current = true
      } catch (err) {
        console.error('[useDataSource] Telemetry fetch error:', err)
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

    fetchTelemetryData()

    // Set up refresh interval if specified (refresh is in seconds, convert to ms)
    const refreshIntervals = telemetryDataSources.map((ds) => ds.refresh).filter(Boolean) as number[]
    const minRefreshSeconds = refreshIntervals.length > 0 ? Math.min(...refreshIntervals) : null

    if (minRefreshSeconds) {
      const minRefreshMs = minRefreshSeconds * 1000
      const interval = setInterval(fetchTelemetryData, minRefreshMs)
      return () => clearInterval(interval)
    }
    // If no periodic refresh, still return cleanup that will re-run on trigger changes
    return () => {}
  }, [telemetryKey, enabled, telemetryRefreshTrigger])

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
