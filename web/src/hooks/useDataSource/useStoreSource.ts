/**
 * useStoreSource — handles store-based data reading, store subscriptions,
 * and device WebSocket event processing for device/metric/command/device-info sources.
 */

import { useEffect, useCallback, useRef, useMemo } from 'react'
import type { DataSource } from '@/types/dashboard'
import { getUnifiedId, getUnifiedField, getUnifiedMode, getUnifiedSource, getEventDeviceId } from '@/types/dashboard'
import type { Device } from '@/types'
import type { NeoMindStore } from '@/store'
import { useStore } from '@/store'
import { useEvents } from '@/hooks/useEvents'
import { logError } from '@/lib/errors'
import {
  extractValueFromData, safeExtractValue, eventMetricMatches,
  getDataSourceLimit, findDevice, buildDeviceMap,
  resolveDeviceInfoValue, insertAndMaintain, isImageDataSource, normalizeImageValue,
} from './helpers'
import { fetchDeviceTelemetry, fetchedDevices, hasActiveFetch, telemetryCache } from './fetch'
import { parseExprRefs, refParts, evalExpression } from '@/lib/expressionSource'
import { processTelemetryEvent, processNonTelemetryEvent, getTs } from './eventProcessors'

// Performance probe — gated by module-level flag to avoid function call overhead in production
const PERF_THRESHOLD = 100
let perfDebugEnabled = false
if (typeof localStorage !== 'undefined') {
  try { perfDebugEnabled = localStorage.getItem('DEBUG_SCROLL') === 'true' } catch { /* SSR */ }
}
function perfMark(label: string) {
  if (!perfDebugEnabled) return
  performance.mark(`uds:${label}`)
}
function perfEnd(label: string) {
  if (!perfDebugEnabled) return
  try {
    performance.measure(`uds:${label}`, `uds:${label}`)
    const entries = performance.getEntriesByName(`uds:${label}`, 'measure')
    const last = entries[entries.length - 1]
    if (last && last.duration > PERF_THRESHOLD) {
      console.warn(`[perf] useDataSource ${label}: ${Math.round(last.duration)}ms`)
    }
  } catch { /* ignore */ }
}

export interface StoreSourceState<T> {
  data: T | null
  setData: (value: T | ((prev: T | null) => T | null)) => void
  setDataRaw: (d: unknown) => void
  setLoading: (l: boolean) => void
  setError: (e: string | null) => void
  setLastUpdate: (ts: number | null) => void
  dataSourcesRef: React.MutableRefObject<DataSource[]>
  optionsRef: React.MutableRefObject<{
    enabled: boolean
    transform?: (data: unknown) => T
    fallback?: T
    preserveMultiple: boolean
  }>
  /** Source-scoped loading adapters for useReducer state machine */
  sourceAdapters?: {
    startLoading: () => void
    finishLoading: () => void
  }
}

/**
 * Process a single telemetry point into currentData.
 * Returns updated data or undefined if no change was needed.
 */
function processSingleTelemetryPoint(
  currentData: unknown,
  newPoint: { timestamp: number; time: number; value: unknown },
  dataSources: DataSource[],
  dsIndex: number,
  ds: DataSource,
  preserveMultiple: boolean,
  maxLimit: number
): unknown | undefined {
  const isImg = isImageDataSource(ds)

  let currentArray: unknown[] = []
  if (Array.isArray(currentData)) {
    if (preserveMultiple && dataSources.length > 1) {
      if (dsIndex >= 0 && Array.isArray((currentData as unknown[])[dsIndex])) {
        currentArray = (currentData as unknown[])[dsIndex] as unknown[]
      }
    } else if (dataSources.length === 1 || !preserveMultiple) {
      currentArray = currentData as unknown[]
    }
  }

  const updated = insertAndMaintain(currentArray, newPoint, getTs, maxLimit, isImg)

  if (preserveMultiple && dataSources.length > 1) {
    // Guard: currentData may be null/undefined on first invocation
    const existing = Array.isArray(currentData) ? currentData as unknown[] : []
    const result = existing.length > 0
      ? existing.map((item, i) => i === dsIndex ? updated : item)
      : dataSources.map((_, i) => i === dsIndex ? updated : [])
    return result
  }
  return updated
}

// ============================================================================
// Global device map cache — shared across all useStoreSource instances
// ============================================================================

let cachedDeviceMapResult: { devicesRef: unknown; map: Map<string, Device> } | null = null

function getCachedDeviceMap(devices: Device[]): Map<string, Device> {
  if (cachedDeviceMapResult && cachedDeviceMapResult.devicesRef === devices) {
    return cachedDeviceMapResult.map
  }
  const map = buildDeviceMap(devices)
  cachedDeviceMapResult = { devicesRef: devices, map }
  return map
}

export function useStoreSource<T = unknown>(
  dataSources: DataSource[],
  dataSourceKey: string,
  enabled: boolean,
  relevantDeviceIds: Set<string>,
  deviceInfoIds: Set<string>,
  hasTelemetrySource: boolean,
  needsWebSocket: boolean,
  state: StoreSourceState<T>,
  hasExtensionSource?: boolean
): { readDataFromStore: () => void; wsConnected: boolean } {
  const initialFetchDoneRef = useRef<Set<string>>(new Set())
  const lastValidDataRef = useRef<Record<string, unknown>>({})

  // Clean up refs when dataSourceKey changes to prevent stale data and memory growth
  const prevSourceKeyRef = useRef(dataSourceKey)
  if (prevSourceKeyRef.current !== dataSourceKey) {
    prevSourceKeyRef.current = dataSourceKey
    initialFetchDoneRef.current.clear()
    lastValidDataRef.current = {}
  }

  const prevStoreStateRef = useRef<{
    rawDevices: NeoMindStore['devices']
    deviceTelemetry: NeoMindStore['deviceTelemetry']
    map: Map<string, Device>
  } | null>(null)

  // Event processing refs
  const processedDeviceEventsRef = useRef<Set<string>>(new Set())
  const lastProcessedDeviceEventIdRef = useRef<string | null>(null)

  // Guard: skip store subscription telemetry merge when WS event effect already
  // processed the same data point (prevents double-processing of telemetry)
  const wsTelemetryProcessedAtRef = useRef(0)

  // Cache for nonTelemetrySources — avoids re-filtering on every readDataFromStore call
  const cachedNonTelRef = useRef<{ dsRef: DataSource[]; result: DataSource[] } | null>(null)
  const getNonTelemetrySources = (dsList: DataSource[]): DataSource[] => {
    if (cachedNonTelRef.current && cachedNonTelRef.current.dsRef === dsList) {
      return cachedNonTelRef.current.result
    }
    const result = dsList.filter(
      (ds) => {
        if (ds.mode === 'timeseries') return false
        if (ds.mode === 'latest' && ds.source === 'expression') return true
        if (ds.mode === 'latest' && ds.source === 'device') return true
        if (ds.mode === 'info' && ds.source === 'device') return true
        if (ds.mode === 'command' && ds.source === 'device') return true
        return false
      }
    )
    cachedNonTelRef.current = { dsRef: dsList, result }
    return result
  }

  // ============================================================================
  // readDataFromStore (device/metric/command/device-info)
  // ============================================================================

  const readDataFromStore = useCallback(() => {
    perfMark('readData')
    const { transform: transformFn, fallback: fallbackVal } = state.optionsRef.current
    const currentDataSources = state.dataSourcesRef.current
    const storeState = useStore.getState()
    const currentDevices = storeState.devices
    const currentTelemetry = storeState.deviceTelemetry

    if (currentDataSources.length === 0) {
      if (fallbackVal !== undefined) state.setData(fallbackVal)
      if (state.sourceAdapters) state.sourceAdapters.finishLoading()
      else state.setLoading(false)
      return
    }

    const nonTelemetrySources = getNonTelemetrySources(currentDataSources)

    // When all sources are telemetry/system/extension, readDataFromStore has nothing to do.
    // Don't touch loading state — those hooks manage their own loading lifecycle.
    if (nonTelemetrySources.length === 0) return

    try {
      const results = nonTelemetrySources.map((ds) => {
        let result: unknown

        // Mode-based routing (all DataSources have unified fields via migrateToUnified)
        const mode = ds.mode
        const source = ds.source

        if (mode === 'latest' && source === 'expression') {
          // Inline computed value: resolve refs from live telemetry, evaluate,
          // fall back to the last valid value while a ref is transiently empty.
          const expr = ds.expr ?? ''
          if (!expr) { result = fallbackVal ?? null; return result }
          const values: Record<string, number> = {}
          for (const ref of parseExprRefs(expr)) {
            const parts = refParts(ref)
            if (!parts) continue
            const cv = currentTelemetry[parts.id]
            const raw = cv && typeof cv === 'object' ? extractValueFromData(cv, parts.field) : undefined
            const num = typeof raw === 'number' ? raw : Number(raw)
            if (Number.isFinite(num)) values[ref] = num
          }
          const evaluated = evalExpression(expr, values)
          const cacheKey = `expr:${expr}`
          if (evaluated.value !== null) {
            lastValidDataRef.current[cacheKey] = evaluated.value
            result = evaluated.value
          } else {
            result = lastValidDataRef.current[cacheKey] ?? '-'
          }
        } else if (mode === 'latest' && source === 'device') {
          const deviceId = getUnifiedId(ds)
          if (!deviceId) { result = fallbackVal ?? null; return result }
          const property = getUnifiedField(ds) as string | undefined
          const device = findDevice(currentDevices, deviceId)
          // Read telemetry from split map, fallback to device.current_values
          const cv = currentTelemetry[deviceId] || device?.current_values

          if (!property) {
            result = device ?? null
          } else {
            const cacheKey = `${deviceId}:${property}`
            if (cv && typeof cv === 'object' && Object.keys(cv).length > 0) {
              const extracted = extractValueFromData(cv, property)
              if (extracted !== undefined) {
                result = extracted
                lastValidDataRef.current[cacheKey] = extracted
              } else {
                let foundNested = false
                for (const nestedKey of ['values', 'metrics', 'data']) {
                  if (cv[nestedKey] && typeof cv[nestedKey] === 'object') {
                    const nestedValue = extractValueFromData(cv[nestedKey] as Record<string, unknown>, property)
                    if (nestedValue !== undefined) { result = nestedValue; foundNested = true; lastValidDataRef.current[cacheKey] = nestedValue; break }
                  }
                }
                if (!foundNested) result = lastValidDataRef.current[cacheKey] ?? '-'
              }
            } else if (device) {
              if (initialFetchDoneRef.current.has(deviceId) || fetchedDevices.has(deviceId) || hasActiveFetch(deviceId)) {
                result = lastValidDataRef.current[cacheKey] ?? '-'
              } else {
                initialFetchDoneRef.current.add(deviceId)
                fetchDeviceTelemetry(deviceId).catch(() => {})
                result = lastValidDataRef.current[cacheKey] ?? '-'
              }
            } else {
              if (initialFetchDoneRef.current.has(deviceId) || hasActiveFetch(deviceId)) {
                result = lastValidDataRef.current[cacheKey] ?? '-'
              } else {
                initialFetchDoneRef.current.add(deviceId)
                import('@/lib/api').then(({ api }) => {
                  api.getDevices().then(() => fetchDeviceTelemetry(deviceId)).catch(() => {})
                })
                result = lastValidDataRef.current[cacheKey] ?? '-'
              }
            }
            result = safeExtractValue(result, '-')
          }
        } else if (mode === 'command' && source === 'device') {
          const deviceId = getUnifiedId(ds)
          if (!deviceId) { result = fallbackVal ?? false; return result }
          const property = getUnifiedField(ds) || 'state'
          const device = findDevice(currentDevices, deviceId)
          const cv = currentTelemetry[deviceId] || device?.current_values
          if (cv && typeof cv === 'object') {
            result = extractValueFromData(cv, property) ?? false
          } else { result = false }
          result = safeExtractValue(result, false)
        } else if (mode === 'info' && source === 'device') {
          const deviceId = getUnifiedId(ds)
          if (!deviceId) { result = fallbackVal ?? '-'; return result }
          const infoProperty = getUnifiedField(ds) || 'name'
          const device = findDevice(currentDevices, deviceId)
          result = resolveDeviceInfoValue(device, infoProperty, fallbackVal)
          result = safeExtractValue(result as unknown, (fallbackVal ?? '-') as any)
        } else {
          result = fallbackVal ?? null
        }
        return result
      })

      let finalData: unknown
      if (nonTelemetrySources.length > 0 && currentDataSources.length > 1) {
        finalData = results
      } else {
        finalData = results[0]
      }

      const transformedData = transformFn ? transformFn(finalData) : (finalData as T)
      state.setData(transformedData)
      state.setLastUpdate(Date.now())
    } catch (err) {
      const { devicesLoading } = useStore.getState()
      if (!devicesLoading) state.setError(err instanceof Error ? err.message : 'Unknown error')
      state.setData((state.optionsRef.current.fallback ?? 0) as T)
    } finally {
      if (state.sourceAdapters) state.sourceAdapters.finishLoading()
      else state.setLoading(false)
      perfEnd('readData')
    }
  }, [])

  // ============================================================================
  // Store subscription (device/metric/command/device-info real-time)
  // ============================================================================

  useEffect(() => {
    const finishStore = state.sourceAdapters
      ? () => state.sourceAdapters!.finishLoading()
      : () => state.setLoading(false)

    if (dataSources.length === 0) {
      const { fallback: fallbackVal } = state.optionsRef.current
      if (fallbackVal !== undefined) state.setData(fallbackVal)
      finishStore()
      return
    }
    if (!enabled) { finishStore(); return }

    // Only create a store subscription when there are device-mode sources
    // (latest/command/info). Telemetry-only components get their data via
    // useTelemetrySource + WS event processing; subscribing to the store
    // would fire on every unrelated device telemetry update.
    const hasDeviceModeSources = dataSources.some(ds => {
      const mode = getUnifiedMode(ds)
      const source = getUnifiedSource(ds)
      return source === 'device' && (mode === 'latest' || mode === 'command' || mode === 'info')
    })
    if (!hasDeviceModeSources) {
      readDataFromStore()
      // readDataFromStore returns early for telemetry-only/extension-only sources
      // without touching loading state — those hooks manage their own loading lifecycle.
      // Finish the initial counter here so their own startLoading/finishLoading
      // pairs stay balanced.
      finishStore()
      return
    }

    readDataFromStore()
    const initialStore = useStore.getState()
    prevStoreStateRef.current = {
      rawDevices: initialStore.devices,
      deviceTelemetry: initialStore.deviceTelemetry,
      map: getCachedDeviceMap(initialStore.devices),
    }

    let unsubscribed = false

    const processStoreChange = (s: NeoMindStore) => {
      if (unsubscribed) return
      try {
      perfMark('storeChange')

      const prev = prevStoreStateRef.current
      if (!prev) return

      // Early exit: nothing changed at all
      if (s.devices === prev.rawDevices && s.deviceTelemetry === prev.deviceTelemetry) return

      const devicesChanged = s.devices !== prev.rawDevices
      const telemetryChanged = s.deviceTelemetry !== prev.deviceTelemetry
      const devicesLengthChanged = s.devices.length !== prev.rawDevices.length
      let currentValuesChanged = false
      const changedDeviceIds = new Set<string>()

      // Only rebuild device map when reference changes
      const currMap = devicesChanged ? getCachedDeviceMap(s.devices) : prev.map

      // Check telemetry changes (per-device diff for relevant devices only)
      if (telemetryChanged && !devicesLengthChanged) {
        for (const deviceId of relevantDeviceIds) {
          const newTel = s.deviceTelemetry[deviceId]
          const prevTel = prev.deviceTelemetry[deviceId]
          if (newTel !== prevTel) {
            currentValuesChanged = true
            changedDeviceIds.add(deviceId)
          }
        }
      }

      // Check device info changes (status, online, last_seen)
      if (!devicesLengthChanged) {
        const prevMap = prev.map
        for (const deviceId of relevantDeviceIds) {
          if (deviceInfoIds.has(deviceId)) {
            const device = currMap.get(deviceId)
            const prevDevice = prevMap.get(deviceId)
            if (device && prevDevice) {
              if (device.status !== prevDevice.status || device.online !== prevDevice.online || device.last_seen !== prevDevice.last_seen) {
                currentValuesChanged = true
                changedDeviceIds.add(deviceId)
              }
            } else if (device !== prevDevice) {
              // Device added or removed
              currentValuesChanged = true
              changedDeviceIds.add(deviceId)
            }
          }
        }
      }

      if (devicesChanged || devicesLengthChanged || currentValuesChanged) {
        let hasRelevantChange = devicesLengthChanged ||
          (devicesChanged && changedDeviceIds.size === 0)
        if (!hasRelevantChange) {
          for (const id of changedDeviceIds) {
            if (relevantDeviceIds.has(id)) { hasRelevantChange = true; break }
          }
        }
        if (!hasRelevantChange && !currentValuesChanged) {
          // Still update ref to avoid re-checking same state
          prevStoreStateRef.current = { rawDevices: s.devices, deviceTelemetry: s.deviceTelemetry, map: currMap }
          return
        }

        prevStoreStateRef.current = { rawDevices: s.devices, deviceTelemetry: s.deviceTelemetry, map: currMap }
        readDataFromStore()

        // Telemetry merge from store changes — build synthetic events for all
        // changed devices/metrics, then process in a single setData call
        const currentDataSources = state.dataSourcesRef.current
        // Only merge telemetry points for timeseries sources, NOT mode='latest'
        // Pre-build index map to avoid O(n) indexOf inside nested loop
        const telSourcesWithIndex: Array<{ ds: DataSource; idx: number }> = []
        for (let i = 0; i < currentDataSources.length; i++) {
          if (currentDataSources[i].mode === 'timeseries') {
            telSourcesWithIndex.push({ ds: currentDataSources[i], idx: i })
          }
        }
        if (telSourcesWithIndex.length > 0 && currentValuesChanged && changedDeviceIds.size > 0) {
          // Skip telemetry merge if WS event effect just processed telemetry data
          // (within last 100ms) to prevent double-processing the same data point
          const skipDueToWsProcessing = (Date.now() - wsTelemetryProcessedAtRef.current) < 100
          if (!skipDueToWsProcessing) {
          const { preserveMultiple: pm, transform: tf } = state.optionsRef.current
          const now = Math.floor(Date.now() / 1000)
          const cacheKeysToInvalidate: string[] = []

          state.setData((prevData) => {
            let currentData = prevData as unknown

            for (const deviceId of changedDeviceIds) {
              // Read telemetry from split map, fallback to device.current_values
              const cv = s.deviceTelemetry[deviceId] || findDevice(s.devices, deviceId)?.current_values
              if (!cv || typeof cv !== 'object') continue

              for (const { ds, idx } of telSourcesWithIndex) {
                // Only device-sourced telemetry can match changedDeviceIds
                if (ds.source !== 'device') continue
                const dsId = getUnifiedId(ds)
                if (dsId !== deviceId) continue
                const metricId = getUnifiedField(ds) || 'value'
                const latestValue = extractValueFromData(cv, metricId)
                if (latestValue === undefined) continue

                // Inline the processTelemetryEvent logic for a single point
                const imgDs = isImageDataSource(ds)
                const normalizedVal = imgDs ? normalizeImageValue(latestValue) : latestValue
                const newPoint = { timestamp: now, time: now, value: normalizedVal }
                const maxLimit = getDataSourceLimit(ds)

                // Update currentData in-place for next iteration
                const updated = processSingleTelemetryPoint(
                  currentData, newPoint, currentDataSources, idx, ds, pm, maxLimit
                )
                if (updated !== undefined) {
                  currentData = updated
                  cacheKeysToInvalidate.push(`${getUnifiedId(ds)}|${getUnifiedField(ds)}|${ds.aggregateExt ?? 'raw'}|`)
                }
              }
            }

            if (currentData === prevData) return prevData
            return (tf ? tf(currentData) : currentData) as T
          })

          // Invalidate cache outside of setData
          for (const prefix of cacheKeysToInvalidate) {
            telemetryCache.deleteWhere((_, key) => key.startsWith(prefix))
          }
          if (cacheKeysToInvalidate.length > 0) {
            state.setLastUpdate(Date.now())
          }
          } // end if (!skipDueToWsProcessing)
        }
      }
      perfEnd('storeChange')
      } catch (err) {
        logError(err, { operation: 'Store subscription change' })
      }
    }

    const unsubscribe = useStore.subscribe((s: NeoMindStore) => {
      processStoreChange(s)
    })

    return () => {
      unsubscribed = true
      unsubscribe()
    }
  }, [dataSources.length, enabled, relevantDeviceIds, deviceInfoIds, hasTelemetrySource, hasExtensionSource])

  // ============================================================================
  // Device WebSocket event processing
  // ============================================================================

  const { events, isConnected: wsConnected } = useEvents({
    enabled: enabled && needsWebSocket,
    category: 'device',
    onConnected: () => {
      processedDeviceEventsRef.current.clear()
      lastProcessedDeviceEventIdRef.current = null
      // Re-fetch data to fill gaps from disconnect period
      if (relevantDeviceIds.size > 0) {
        readDataFromStore()
      }
    },
  })


  const eventsKey = useMemo(() => {
    if (events.length === 0) return 'empty'
    const lastEvent = events[events.length - 1]
    return `events-${events.length}-${lastEvent?.id || 'unknown'}`
  }, [events])

  useEffect(() => {
    if (dataSources.length === 0 || !enabled || events.length === 0) return
    perfMark('events')

    let startIndex = 0
    const lastProcessedId = lastProcessedDeviceEventIdRef.current
    if (lastProcessedId) {
      // Backward scan — cache-friendly, stops at first match
      let found = -1
      for (let i = events.length - 1; i >= 0; i--) {
        if (events[i].id === lastProcessedId) { found = i; break }
      }
      if (found !== -1) startIndex = found + 1
      else { startIndex = 0; const entries = Array.from(processedDeviceEventsRef.current); processedDeviceEventsRef.current = new Set(entries.slice(-50)) }
    }
    if (startIndex > events.length) { startIndex = 0; processedDeviceEventsRef.current.clear() }

    const newEvents = events.slice(startIndex)
    if (newEvents.length === 0) return

    // Pre-compute outside event loop — avoids per-event scan
    const currentDataSources = state.dataSourcesRef.current
    // Use the prop rather than re-scanning; hasTelemetrySource is derived from
    // the same dataSources via useMemo and doesn't change between renders
    const hasTelemetrySrc = hasTelemetrySource

    // Build deviceId → telemetry sources map for O(1) lookup per event.
    // Use getEventDeviceId so transform/ai sources are keyed by their
    // WS event namespace ("transform:{id}"), matching backend event device_id.
    const telemetryByDevice = new Map<string, DataSource[]>()
    if (hasTelemetrySrc) {
      for (const ds of currentDataSources) {
        if (ds.mode !== 'timeseries') continue
        const dsId = getEventDeviceId(ds)
        if (!dsId) continue
        const existing = telemetryByDevice.get(dsId)
        if (existing) existing.push(ds)
        else telemetryByDevice.set(dsId, [ds])
      }
    }

    let lastProcessedIdInBatch: string | null = null

    for (const latestEvent of newEvents) {
      const eventData = (latestEvent as any).data || latestEvent
      const eventType = (latestEvent as any).type

      const hasDeviceId = eventData && typeof eventData === 'object' && 'device_id' in eventData
      if (hasDeviceId && relevantDeviceIds.size > 0) {
        if (!relevantDeviceIds.has(eventData.device_id as string)) continue
      }

      // Deterministic event ID: use event content hash to avoid duplicates
      // even when event.id is missing
      const valueKey = hasDeviceId ? (typeof eventData.value === 'object' && eventData.value !== null ? `obj:${(eventData.value as any).type || ''}:${String(eventData.value).slice(0, 80)}` : String(eventData.value ?? '')) : ''
      const uniqueEventId = latestEvent.id || `${eventType}_${eventData.device_id || ''}_${eventData.metric || ''}_${eventData.timestamp || ''}_${valueKey}`
      if (processedDeviceEventsRef.current.has(uniqueEventId)) continue
      processedDeviceEventsRef.current.add(uniqueEventId)
      lastProcessedIdInBatch = uniqueEventId

      if (processedDeviceEventsRef.current.size > 100) {
        const entries = Array.from(processedDeviceEventsRef.current)
        processedDeviceEventsRef.current = new Set(entries.slice(-50))
      }

      const normalizedEventType = eventType?.toLowerCase().replace('.', '')
      const isDeviceMetricEvent = normalizedEventType?.includes('devicemetric') ||
                                   normalizedEventType?.includes('metric') || eventType === 'DeviceMetric'
      const eventMetric = typeof (eventData as any).metric === 'string' ? (eventData as any).metric : ''

      let shouldUpdate = false

      // Update store metrics — skip virtual metrics (transform/extension outputs)
      // to prevent polluting device current_values with virtual metric data.
      const isVirtualMetric = eventData.is_virtual === true
      if (isDeviceMetricEvent && hasDeviceId && !isVirtualMetric) {
        const deviceId = eventData.device_id as string
        const store = useStore.getState()
        if ('metric' in eventData && 'value' in eventData) {
          store.updateDeviceMetric(deviceId, eventData.metric as string, eventData.value)
        }
        for (const [key, value] of Object.entries(eventData)) {
          if (key !== 'device_id' && key !== 'timestamp' && key !== 'type' && key !== 'id' && key !== 'metric' && key !== 'value' && key !== 'is_virtual') {
            store.updateDeviceMetric(deviceId, key, value)
          }
        }
        shouldUpdate = true
      }

      // Check if event matches data sources.
      // Use getEventDeviceId for matching so transform/ai sources match
      // events published with "transform:{id}" / "ai:{id}" device_id.
      for (const ds of currentDataSources) {
        const dsEventId = getEventDeviceId(ds)
        const dsField = getUnifiedField(ds) ?? 'value'
        const dsMode = getUnifiedMode(ds)
        const dsSource = getUnifiedSource(ds)

        // Mode-based matching
        if (dsMode === 'latest' && dsSource === 'device' && hasDeviceId && eventData.device_id === dsEventId && isDeviceMetricEvent) { shouldUpdate = true; break }
        else if (dsMode === 'command' && dsSource === 'device' && hasDeviceId && eventData.device_id === dsEventId && (isDeviceMetricEvent || eventType === 'device.command_result')) { shouldUpdate = true; break }
        else if (dsMode === 'timeseries' && hasDeviceId && eventData.device_id === dsEventId && isDeviceMetricEvent && (!eventMetric || eventMetricMatches(eventMetric, dsField))) { shouldUpdate = true; break }
        else if (dsMode === 'info' && dsSource === 'device' && hasDeviceId && eventData.device_id === dsEventId && (isDeviceMetricEvent || eventType === 'DeviceOnline' || eventType === 'DeviceOffline')) { shouldUpdate = true; break }
      }

      // Telemetry merge (optimized path) — only for timeseries mode
      let telemetryAlreadyProcessed = false

      if (hasTelemetrySrc && isDeviceMetricEvent && hasDeviceId) {
        const eventDeviceId = eventData.device_id as string
        const deviceTelSources = telemetryByDevice.get(eventDeviceId)
        const matchingSources = deviceTelSources
          ? deviceTelSources.filter((ds) => {
              if (!eventMetric) return true
              const metricId = getUnifiedField(ds) || 'value'
              return eventMetric === metricId || eventMetricMatches(eventMetric, metricId)
            })
          : []

        if (matchingSources.length > 0) {
          telemetryAlreadyProcessed = true
          // Mark that WS just processed telemetry — store subscription will skip
          // its telemetry merge for a brief window to avoid double-processing
          wsTelemetryProcessedAtRef.current = Date.now()
          const { preserveMultiple: pm, transform: tf } = state.optionsRef.current
          processTelemetryEvent(eventData, eventMetric, eventDeviceId, currentDataSources, pm, tf, (updater) => state.setData((prev) => updater(prev) as T), state.setLastUpdate)

          // Mark cache entries as stale — include aggregate in prefix to avoid
          // deleting entries for other components with different aggregation
          matchingSources.forEach((ds) => {
            const keyPrefix = `${getEventDeviceId(ds)}|${getUnifiedField(ds)}|`
            telemetryCache.deleteWhere((_, key) => key.startsWith(keyPrefix))
          })
        }
      }

      // Non-telemetry event processing
      if (shouldUpdate && !telemetryAlreadyProcessed) {
        const { preserveMultiple: pm, transform: tf, fallback: fb } = state.optionsRef.current
        processNonTelemetryEvent(eventData, eventType, isDeviceMetricEvent, eventMetric, hasDeviceId, currentDataSources, pm, tf, fb, (updater) => state.setData((prev) => updater(prev) as T), state.setLastUpdate)
      }
    }

    if (lastProcessedIdInBatch) lastProcessedDeviceEventIdRef.current = lastProcessedIdInBatch
    perfEnd('events')
  }, [enabled, dataSourceKey, eventsKey])

  // Expose readDataFromStore for useTelemetrySource and wsConnected for polling fallback
  return { readDataFromStore, wsConnected }
}
