/**
 * useDataSource Hook (Refactored)
 *
 * Simplified data binding for dashboard components.
 * - Removed debug console.logs
 * - Extracted data utilities to separate module
 * - Cleaner event handling
 */

import { useEffect, useState, useCallback, useRef, useMemo } from 'react'
import type { DataSourceOrList, DataSource } from '@/types/dashboard'
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
  aggregate: 'raw' | 'avg' | 'min' | 'max' | 'sum' = 'raw',
  includeRawPoints: boolean = false
): Promise<{ data: number[]; raw?: any[]; success: boolean }> {
  const cacheKey = `${deviceId}|${metricId}|${timeRange}|${limit}|${aggregate}`
  const cached = telemetryCache.get(cacheKey)

  // Return cached data if fresh
  if (cached && Date.now() - cached.timestamp < TELEMETRY_CACHE_TTL) {
    return { data: cached.data, raw: cached.raw, success: true }
  }

  try {
    const api = (await import('@/lib/api')).api
    const now = Date.now()
    const start = now - timeRange * 60 * 60 * 1000

    const response = await api.getDeviceTelemetry(deviceId, metricId, Math.floor(start / 1000), Math.floor(now / 1000), limit)

    // TelemetryDataResponse has structure: { data: Record<string, TelemetryPoint[]> }
    // The actual time series data is in response.data[metricId]
    if (response?.data && typeof response.data === 'object') {
      const metricData = response.data[metricId]

      if (Array.isArray(metricData) && metricData.length > 0) {
        // Extract values from telemetry points
        const values = metricData
          .map((point) => {
            if (typeof point === 'number') return point
            if (typeof point === 'object' && point !== null) {
              const p = point as unknown as Record<string, unknown>
              // Try common value field names
              return (p.value ?? p.v ?? p.avg ?? p.min ?? p.max ?? 0) as number
            }
            return 0
          })
          .filter((v: number) => typeof v === 'number' && !isNaN(v))

        // Cache the result with raw points if requested
        telemetryCache.set(cacheKey, {
          data: values,
          raw: includeRawPoints ? metricData : undefined,
          timestamp: Date.now()
        })

        return { data: values, raw: includeRawPoints ? metricData : undefined, success: true }
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

export function useDataSource<T = unknown>(
  dataSource: DataSourceOrList | undefined,
  options?: {
    enabled?: boolean
    transform?: (data: unknown) => T
    fallback?: T
  }
): UseDataSourceResult<T> {
  const { enabled = true, transform, fallback } = options ?? {}
  const [data, setData] = useState<T | null>(fallback ?? null)
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)
  const [lastUpdate, setLastUpdate] = useState<number | null>(null)
  const [sending, setSending] = useState(false)

  // CRITICAL: Memoize dataSources to prevent infinite re-renders
  // Using JSON.stringify of dataSource as the key ensures stability
  const dataSourceKey = useMemo(() => {
    return JSON.stringify(dataSource)
  }, [dataSource])

  const dataSources = useMemo(() => {
    return dataSource ? normalizeDataSource(dataSource) : []
  }, [dataSourceKey])

  const initialFetchDoneRef = useRef<Set<string>>(new Set())
  const lastValidDataRef = useRef<Record<string, unknown>>({})

  const optionsRef = useRef({ enabled, transform, fallback })
  optionsRef.current = { enabled, transform, fallback }

  const dataSourcesRef = useRef(dataSources)
  dataSourcesRef.current = dataSources

  // Check for command source
  const hasCommandSource = dataSources.some((ds) => ds.type === 'command')
  const commandSource = dataSources.find((ds) => ds.type === 'command')

  // Send command function
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

      setData(value as T)
      setLastUpdate(Date.now())
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
      const results = currentDataSources.map((ds) => {
        let result: unknown

        switch (ds.type) {
          case 'static':
            result = ds.staticValue
            break

          case 'device': {
            const deviceId = ds.deviceId!
            const property = (ds.property as string | undefined) || 'value'
            const device = currentDevices.find((d: any) => d.id === deviceId || d.device_id === deviceId)
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

          case 'telemetry': {
            // Telemetry data is fetched asynchronously and stored in state
            // Initial result is empty array, will be populated by fetch effect
            result = []
            break
          }

          default:
            result = fallbackVal ?? null
        }

        return result
      })

      // Combine results
      let finalData: unknown
      if (currentDataSources.length > 1) {
        finalData = results
      } else {
        finalData = results[0]
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
    if (dataSources.length === 0 || !enabled) {
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
  const needsWebSocket = dataSources.some((ds) =>
    ds.type === 'websocket' || ds.type === 'device' || ds.type === 'metric' || ds.type === 'command'
  )

  const { events } = useEvents({
    enabled: enabled && needsWebSocket,
    eventTypes: ['DeviceMetric', 'DeviceStatus', 'DeviceCommandResult'] as any,
  })

  useEffect(() => {
    if (dataSources.length === 0 || !enabled || events.length === 0) return

    const latestEvent = events[events.length - 1]
    if (!latestEvent) return

    const eventData = (latestEvent as any).data || latestEvent
    const eventType = (latestEvent as any).type

    const isDeviceMetricEvent = eventType === 'device.metric' || eventType === 'DeviceMetric'
    const hasDeviceId = eventData && typeof eventData === 'object' && 'device_id' in eventData

    let shouldUpdate = false

    // Update store for device metric events
    if (isDeviceMetricEvent && hasDeviceId) {
      const deviceId = eventData.device_id as string
      const store = useStore.getState()
      for (const [key, value] of Object.entries(eventData)) {
        if (key !== 'device_id' && key !== 'timestamp' && key !== 'type' && key !== 'id') {
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
            const property = (ds.property as string | undefined) || 'value'

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
      .map((ds) => JSON.stringify({ deviceId: ds.deviceId, metricId: ds.metricId, timeRange: ds.timeRange, limit: ds.limit, aggregate: ds.aggregate }))
      .join('|')
  }, [dataSources])

  const telemetryDataSources = useMemo(() => {
    return dataSources.filter((ds) => ds.type === 'telemetry')
  }, [dataSources])

  const hasTelemetrySource = telemetryDataSources.length > 0

  useEffect(() => {
    if (!hasTelemetrySource || !enabled) return

    // Track if initial fetch has completed to avoid showing loading on refreshes
    let initialFetchCompleted = false

    const fetchTelemetryData = async () => {
      // Only show loading state on initial fetch, not on interval refreshes
      if (!initialFetchCompleted) {
        setLoading(true)
      }
      setError(null)

      try {
        const results = await Promise.all(
          telemetryDataSources.map(async (ds) => {
            if (!ds.deviceId || !ds.metricId) {
              console.warn('[useDataSource] Missing deviceId or metricId:', ds)
              return { data: [], raw: undefined }
            }

            // Check if raw points are needed (for image history, etc.)
            const includeRawPoints = ds.params?.includeRawPoints === true || ds.transform === 'raw'

            const response = await fetchHistoricalTelemetry(
              ds.deviceId,
              ds.metricId,
              ds.timeRange ?? 1,
              ds.limit ?? 50,
              ds.aggregate ?? 'raw',
              includeRawPoints
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
          // Check if any result has raw data
          const hasRawData = results.some((r: any) => r.raw)
          if (hasRawData) {
            // Combine raw data from all sources
            const allRawData = results.flatMap((r: any) => r.raw ?? [])
            finalData = allRawData
          } else {
            finalData = results.map((r: any) => r.data ?? []).flat()
          }
        } else {
          const singleResult = results[0] as any
          finalData = singleResult.raw ?? singleResult.data ?? []
        }

        const { transform: transformFn } = optionsRef.current
        const transformedData = transformFn ? transformFn(finalData) : (finalData as T)
        setData(transformedData)
        setLastUpdate(Date.now())
        initialFetchCompleted = true
      } catch (err) {
        console.error('[useDataSource] Telemetry fetch error:', err)
        const errorMessage = err instanceof Error ? err.message : 'Failed to fetch telemetry'
        setError(errorMessage)
        const fallbackData = optionsRef.current.fallback ?? []
        setData(fallbackData as T)
        initialFetchCompleted = true
      } finally {
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
  }, [telemetryKey, enabled])

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
