/**
 * Pure functions extracted from useDataSource for event processing and data manipulation.
 * No React imports — these are stateless utilities.
 */

import type { DataSource } from '@/types/dashboard'
import { getUnifiedId, getUnifiedField, getUnifiedMode, getUnifiedSource, getEventDeviceId } from '@/types/dashboard'
import { useStore } from '@/store'
import {
  extractValueFromData, safeExtractValue, eventMetricMatches,
  getPointValue, isImageDataSource, getDataSourceLimit,
  isDuplicatePoint, sortAndDedup, normalizeImageValue,
  findDevice, resolveDeviceInfoValue, insertAndMaintain,
} from './helpers'
import { timeWindowToHours } from '@/lib/telemetryTransform'

// ============================================================================
// Timestamp utilities
// ============================================================================

export const getTs = (p: unknown): number => {
  if (p == null) return 0
  if (typeof p === 'number') return 0
  const o = p as Record<string, unknown>
  return (o.timestamp ?? o.time ?? o.t ?? 0) as number
}

/**
 * Get the newest timestamp from telemetry data (handles both flat arrays
 * and nested preserveMultiple arrays).
 */
export function getNewestTimestamp(data: unknown, tsFn: (p: unknown) => number): number {
  if (!Array.isArray(data)) return 0
  if (data.length === 0) return 0

  // Nested arrays (preserveMultiple)
  if (Array.isArray(data[0])) {
    let max = 0
    for (const arr of data as unknown[][]) {
      for (const p of arr) { const ts = tsFn(p); if (ts > max) max = ts }
    }
    return max
  }

  let max = 0
  for (const p of data) { const ts = tsFn(p); if (ts > max) max = ts }
  return max
}

/**
 * Extract live points from prevData that are strictly newer than `afterTs`.
 */
export function extractPointsNewerThan(
  prevData: unknown, afterTs: number,
  tsFn: (p: unknown) => number,
  preserveMultiple: boolean, sourceCount: number
): unknown[][] {
  if (!Array.isArray(prevData)) return []
  if (preserveMultiple && sourceCount > 1 && Array.isArray(prevData[0])) {
    return (prevData as unknown[][]).map(arr => {
      if (!Array.isArray(arr)) return []
      return arr.filter(p => tsFn(p) > afterTs)
    })
  }
  const live = (prevData as unknown[]).filter(p => tsFn(p) > afterTs)
  return live.length > 0 ? [live] : []
}

/**
 * Merge live points back into fetched data, deduplicating by timestamp.
 */
export function mergeLiveData(
  fetchedData: unknown, livePoints: unknown[][],
  tsFn: (p: unknown) => number,
  preserveMultiple: boolean, sources: DataSource[]
): unknown {
  if (preserveMultiple && sources.length > 1 && Array.isArray(fetchedData)) {
    return sources.map((ds, i) => {
      const fetched = (fetchedData as unknown[][])[i] ?? []
      const live = livePoints[i] ?? []
      const isImg = isImageDataSource(ds)
      const maxLimit = getDataSourceLimit(ds)
      // Fetched data is already sorted & deduplicated by sortTelemetryResults.
      // Only live WS points need individual insertion — avoids O(n²) array copies.
      let result = fetched
      for (const p of live) result = insertAndMaintain(result, p, tsFn, maxLimit, isImg)
      return result
    })
  }
  const fetched = Array.isArray(fetchedData) ? fetchedData : []
  const live = livePoints[0] ?? []
  const ds = sources[0]
  const maxLimit = ds ? getDataSourceLimit(ds) : 50
  const isImg = ds ? isImageDataSource(ds) : false
  let result = fetched
  for (const p of live) result = insertAndMaintain(result, p, tsFn, maxLimit, isImg)
  return result
}

// ============================================================================
// Sorting helpers
// ============================================================================

export function normalizeOutputName(outputName: string): string {
  if (!outputName.includes(':')) return outputName
  return outputName.split(':').slice(1).join(':')
}

export function sortTelemetryResults(finalData: unknown, sources: DataSource[], preserveMultiple: boolean): unknown {
  const isPM = preserveMultiple && sources.length > 1
  const process = (points: unknown[], ds: DataSource): unknown[] => {
    if (!Array.isArray(points) || points.length === 0) return points
    const isImg = isImageDataSource(ds)
    const maxLimit = getDataSourceLimit(ds)
    // Sort descending (newest-first), dedup, then reverse to ascending (oldest-first)
    const sorted = sortAndDedup(points, getTs, maxLimit, isImg)
    return sorted.slice().reverse()
  }
  if (isPM && Array.isArray(finalData)) return sources.map((ds, i) => process((finalData as unknown[][])[i], ds))
  if (Array.isArray(finalData) && sources.length > 0) return process(finalData, sources[0])
  return finalData
}

// ============================================================================
// Event processors
// ============================================================================

export function processTelemetryEvent(
  eventData: any, eventMetric: string, eventDeviceId: string,
  dataSources: DataSource[], preserveMultiple: boolean,
  transform: ((data: unknown) => unknown) | undefined,
  setData: (updater: (prev: unknown) => unknown) => void, setLastUpdate: (ts: number) => void
) {
  const now = Math.floor(Date.now() / 1000)
  const rawEventTimestamp = eventData.timestamp
  const eventTimestamp = rawEventTimestamp !== undefined
    ? (typeof rawEventTimestamp === 'number' && rawEventTimestamp > 10000000000
        ? Math.floor(rawEventTimestamp / 1000) : rawEventTimestamp)
    : now

  // Fast path: single source (covers ~90% of dashboard components)
  if (dataSources.length === 1) {
    const ds = dataSources[0]
    if (ds.mode !== 'timeseries' || getEventDeviceId(ds) !== eventDeviceId) return

    const dsTimeRangeHours = ds.timeWindow
      ? timeWindowToHours(ds.timeWindow.type)
      : (ds.timeRange ?? 1)
    if (eventTimestamp < now - Math.floor(dsTimeRangeHours * 3600)) return

    const metricId = getUnifiedField(ds) || 'value'
    let eventValue: unknown
    const metricMatches = eventMetric === metricId || eventMetricMatches(eventMetric, metricId)
    if ('value' in eventData && metricMatches) { eventValue = eventData.value }
    else if (!eventMetric) { eventValue = extractValueFromData(eventData, metricId) }
    else return
    if (eventValue === undefined) return

    const isImg = isImageDataSource(ds)
    const maxLimit = getDataSourceLimit(ds)
    const normalizedValue = isImg ? normalizeImageValue(eventValue) : eventValue
    const newPoint = { timestamp: eventTimestamp, time: eventTimestamp, value: normalizedValue }

    setData((prevData: unknown) => {
      const currentArray = Array.isArray(prevData) ? prevData as unknown[] : []
      if (isImg && isDuplicatePoint(currentArray, eventTimestamp, normalizedValue, getTs)) return prevData
      const updated = insertAndMaintain(currentArray, newPoint, getTs, maxLimit, isImg)
      return transform ? transform(updated) : updated
    })
    setLastUpdate(Date.now())
    return
  }

  // Multi-source path: pre-compute matches outside updater
  const matchedResults = dataSources.map((ds) => {
    if (ds.mode !== 'timeseries' || getEventDeviceId(ds) !== eventDeviceId) return undefined
    const dsTimeRangeHours = ds.timeWindow
      ? timeWindowToHours(ds.timeWindow.type)
      : (ds.timeRange ?? 1)
    if (eventTimestamp < now - Math.floor(dsTimeRangeHours * 3600)) return undefined

    const metricId = getUnifiedField(ds) || 'value'
    let eventValue: unknown
    const metricMatches = eventMetric === metricId || eventMetricMatches(eventMetric, metricId)
    if ('value' in eventData && metricMatches) { eventValue = eventData.value }
    else if (!eventMetric) { eventValue = extractValueFromData(eventData, metricId) }
    else return undefined
    if (eventValue === undefined) return undefined

    return { eventValue, isImg: isImageDataSource(ds) }
  })

  if (!matchedResults.some(Boolean)) return

  setData((prevData: unknown) => {
    const updatedResults = dataSources.map((ds, index) => {
      const matched = matchedResults[index]
      if (!matched) return undefined

      const normalizedVal = matched.isImg ? normalizeImageValue(matched.eventValue) : matched.eventValue
      const newPoint = { timestamp: eventTimestamp, time: eventTimestamp, value: normalizedVal }
      let currentArray: unknown[] = []
      if (Array.isArray(prevData)) {
        if (preserveMultiple && dataSources.length > 1 && Array.isArray((prevData as unknown[])[index])) {
          currentArray = (prevData as unknown[])[index] as unknown[]
        } else { currentArray = prevData as unknown[] }
      }

      if (matched.isImg && isDuplicatePoint(currentArray, eventTimestamp, normalizedVal, getTs)) return undefined
      return insertAndMaintain(currentArray, newPoint, getTs, getDataSourceLimit(ds), matched.isImg)
    })

    if (!updatedResults.some(Boolean)) return prevData

    let finalData: unknown
    if (preserveMultiple && dataSources.length > 1) {
      finalData = updatedResults.map((r, i) => r !== undefined ? r : (Array.isArray(prevData) && (prevData as unknown[])[i] !== undefined ? (prevData as unknown[])[i] : []))
    } else {
      const first = updatedResults.find((r) => r !== undefined)
      finalData = first ?? prevData
    }

    return transform ? transform(finalData) : finalData
  })
  setLastUpdate(Date.now())
}

export function processNonTelemetryEvent(
  eventData: any, eventType: string, isDeviceMetricEvent: boolean, eventMetric: string,
  hasDeviceId: boolean, dataSources: DataSource[],
  preserveMultiple: boolean, transform: ((data: unknown) => unknown) | undefined,
  fallback: unknown, setData: (updater: (prev: unknown) => unknown) => void, setLastUpdate: (ts: number) => void
) {
  const storeState = useStore.getState()
  const currentDevices = storeState.devices
  const currentTelemetry = storeState.deviceTelemetry

  // Fast path: single source (covers ~90% of dashboard components)
  if (dataSources.length === 1) {
    const ds = dataSources[0]
    const mode = getUnifiedMode(ds)
    const source = getUnifiedSource(ds)
    let result: unknown

    if (mode === 'latest' && source === 'device') {
      const deviceId = getUnifiedId(ds)
      if (!deviceId) { setData(() => transform ? transform(fallback) : fallback); setLastUpdate(Date.now()); return }
      const property = getUnifiedField(ds) as string | undefined
      if (!property) {
        result = findDevice(currentDevices, deviceId) ?? null
      } else if (isDeviceMetricEvent && eventData.device_id === deviceId) {
        const metricMatches = eventMetric === property || eventMetricMatches(eventMetric, property)
        if ('metric' in eventData && 'value' in eventData && metricMatches) { result = eventData.value }
        else if (!eventMetric) { const extracted = extractValueFromData(eventData, property); if (extracted !== undefined) result = extracted }
        if (result === undefined) {
          const cv = currentTelemetry[deviceId] || findDevice(currentDevices, deviceId)?.current_values
          result = cv ? (extractValueFromData(cv, property) ?? '-') : '-'
        }
      } else {
        const cv = currentTelemetry[deviceId] || findDevice(currentDevices, deviceId)?.current_values
        result = cv ? (extractValueFromData(cv, property) ?? '-') : '-'
      }
      result = safeExtractValue(result, '-')
    } else if (mode === 'command' && source === 'device') {
      const deviceId = getUnifiedId(ds)
      if (!deviceId) { setData(() => transform ? transform(fallback ?? false) : fallback ?? false); setLastUpdate(Date.now()); return }
      const property = getUnifiedField(ds) || 'state'
      if (isDeviceMetricEvent && eventData.device_id === deviceId) {
        const metricMatches = eventMetric === property || eventMetricMatches(eventMetric, property)
        if ('metric' in eventData && 'value' in eventData && metricMatches) { result = eventData.value }
        else if (!eventMetric) { const extracted = extractValueFromData(eventData, property); if (extracted !== undefined) result = extracted }
      }
      if (result === undefined) {
        const cv = currentTelemetry[deviceId] || findDevice(currentDevices, deviceId)?.current_values
        result = cv ? (extractValueFromData(cv, property) ?? false) : false
      }
      result = safeExtractValue(result, false)
    } else if (mode === 'info' && source === 'device') {
      const deviceId = getUnifiedId(ds)
      if (!deviceId) { setData(() => transform ? transform(fallback ?? '-') : fallback ?? '-'); setLastUpdate(Date.now()); return }
      const infoProperty = getUnifiedField(ds) || 'name'
      const device = findDevice(currentDevices, deviceId)
      result = resolveDeviceInfoValue(device, infoProperty, fallback)
      result = safeExtractValue(result as unknown, (fallback ?? '-') as any)
    } else if (mode === 'latest' && source === 'system') {
      result = fallback ?? null
    } else if (mode === 'timeseries') {
      // Timeseries is handled by processTelemetryEvent; preserve existing
      return
    } else {
      // Unhandled — preserve existing
      return
    }

    const finalData = transform ? transform(result) : result
    setData(() => finalData)
    setLastUpdate(Date.now())
    return
  }

  // Multi-source path
  setData((prevData: unknown) => {
    const results = dataSources.map((ds, index) => {
      let result: unknown

      // Mode-based routing (Phase 2 — before legacy type switch)
      const mode = getUnifiedMode(ds)
      const source = getUnifiedSource(ds)

      if (mode === 'latest' && source === 'device') {
        const deviceId = getUnifiedId(ds)
        if (!deviceId) return fallback
        const property = getUnifiedField(ds) as string | undefined
        if (!property) {
          result = findDevice(currentDevices, deviceId) ?? null
        } else if (isDeviceMetricEvent && eventData.device_id === deviceId) {
          const metricMatches = eventMetric === property || eventMetricMatches(eventMetric, property)
          if ('metric' in eventData && 'value' in eventData && metricMatches) { result = eventData.value }
          else if (!eventMetric) { const extracted = extractValueFromData(eventData, property); if (extracted !== undefined) result = extracted }
          if (result === undefined) {
            const cv = currentTelemetry[deviceId] || findDevice(currentDevices, deviceId)?.current_values
            result = cv ? (extractValueFromData(cv, property) ?? '-') : '-'
          }
        } else {
          const cv = currentTelemetry[deviceId] || findDevice(currentDevices, deviceId)?.current_values
          result = cv ? (extractValueFromData(cv, property) ?? '-') : '-'
        }
        result = safeExtractValue(result, '-')
        return result
      }

      if (mode === 'command' && source === 'device') {
        const deviceId = getUnifiedId(ds)
        if (!deviceId) return fallback ?? false
        const property = getUnifiedField(ds) || 'state'
        if (isDeviceMetricEvent && eventData.device_id === deviceId) {
          const metricMatches = eventMetric === property || eventMetricMatches(eventMetric, property)
          if ('metric' in eventData && 'value' in eventData && metricMatches) { result = eventData.value }
          else if (!eventMetric) { const extracted = extractValueFromData(eventData, property); if (extracted !== undefined) result = extracted }
        }
        if (result === undefined) {
          const cv = currentTelemetry[deviceId] || findDevice(currentDevices, deviceId)?.current_values
          result = cv ? (extractValueFromData(cv, property) ?? false) : false
        }
        result = safeExtractValue(result, false)
        return result
      }

      if (mode === 'info' && source === 'device') {
        const deviceId = getUnifiedId(ds)
        if (!deviceId) return fallback ?? '-'
        const infoProperty = getUnifiedField(ds) || 'name'
        const device = findDevice(currentDevices, deviceId)
        result = resolveDeviceInfoValue(device, infoProperty, fallback)
        result = safeExtractValue(result as unknown, (fallback ?? '-') as any)
        return result
      }

      if (mode === 'latest' && source === 'system') {
        result = fallback ?? null
        return result
      }

      if (mode === 'timeseries') {
        if (Array.isArray(prevData) && (prevData as unknown[])[index] !== undefined) {
          result = dataSources.length > 1 ? (prevData as unknown[])[index] : prevData
        } else { result = fallback ?? [] }
        return result
      }

      // Unhandled source/mode combination — preserve existing data instead of
      // returning undefined (which would corrupt multi-source arrays)
      if (Array.isArray(prevData) && (prevData as unknown[])[index] !== undefined) {
        return dataSources.length > 1 ? (prevData as unknown[])[index] : prevData
      }
      return fallback
    })

    const finalData: unknown = dataSources.length > 1 ? results : results[0]
    return transform ? transform(finalData) : finalData
  })
  setLastUpdate(Date.now())
}
