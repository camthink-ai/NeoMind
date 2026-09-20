/**
 * API calls, caching, and in-flight deduplication for useDataSource.
 * Consolidates telemetryFetch.ts, systemFetch.ts, cache.ts, and batchFetch.ts.
 */

import type { TelemetryAggregate, TimeWindowConfig, DataSource } from '@/types/dashboard'
import { getUnifiedField } from '@/types/dashboard'
import { useStore } from '@/store'
import { logError, isNetworkError } from '@/lib/errors'
import { getTimeRange } from '@/lib/telemetryTransform'
import { normalizeImageValue } from './helpers'

// ============================================================================
// Cache (replaces TypedCache class instances)
// ============================================================================

interface CacheEntry<T> { data: T; timestamp: number; meta?: Record<string, unknown> }

function createCache<T>(ttl: number, maxSize: number) {
  const cache = new Map<string, CacheEntry<T>>()

  return {
    get(key: string): T | undefined {
      const entry = cache.get(key)
      if (!entry) return undefined
      if (Date.now() - entry.timestamp > ttl) { cache.delete(key); return undefined }
      return entry.data
    },
    getWithMeta(key: string): { data: T; meta?: Record<string, unknown> } | undefined {
      const entry = cache.get(key)
      if (!entry) return undefined
      if (Date.now() - entry.timestamp > ttl) { cache.delete(key); return undefined }
      return { data: entry.data, meta: entry.meta }
    },
    set(key: string, data: T, meta?: Record<string, unknown>): void {
      if (cache.size >= maxSize) { const k = cache.keys().next().value; if (k) cache.delete(k) }
      cache.set(key, { data, timestamp: Date.now(), meta })
    },
    updateMeta(key: string, meta: Record<string, unknown>): boolean {
      const entry = cache.get(key)
      if (!entry) return false
      entry.meta = meta
      return true
    },
    delete(key: string): boolean { return cache.delete(key) },
    deleteWhere(predicate: (meta: Record<string, unknown> | undefined, key: string) => boolean): void {
      for (const [key, entry] of cache) {
        if (predicate(entry.meta, key)) cache.delete(key)
      }
    },
    cleanup(): void {
      const now = Date.now()
      for (const [key, entry] of cache) {
        if (now - entry.timestamp > ttl) cache.delete(key)
      }
    },
  }
}

const telemetryCache = createCache<{ data: number[]; raw?: unknown[] }>(30000, 100)
const systemStatsCache = createCache<unknown>(30000, 40)
export const extensionDataCache = createCache<unknown>(30000, 100)

let cacheCleanupInterval: ReturnType<typeof setInterval> | null = null
if (typeof window !== 'undefined') {
  cacheCleanupInterval = setInterval(() => {
    telemetryCache.cleanup()
    systemStatsCache.cleanup()
    extensionDataCache.cleanup()
  }, 60000)
  window.addEventListener('beforeunload', () => {
    if (cacheCleanupInterval) { clearInterval(cacheCleanupInterval); cacheCleanupInterval = null }
  })
}

export function clearGlobalCacheIntervals() {
  if (cacheCleanupInterval) { clearInterval(cacheCleanupInterval); cacheCleanupInterval = null }
}

/** Clear all telemetry cache entries — forces fresh fetch on next data request */
export function clearTelemetryCache() {
  telemetryCache.deleteWhere(() => true)
}

// ============================================================================
// Shared extractors for telemetry data points (module scope for reuse)
// ============================================================================

const extractValue = (point: unknown): number => {
  if (typeof point === 'number') return point
  if (typeof point === 'object' && point !== null) {
    const p = point as Record<string, unknown>
    const rawValue = p.value ?? p.v ?? p.avg ?? p.min ?? p.max ?? 0
    if (typeof rawValue === 'number') return rawValue
    if (typeof rawValue === 'string') { const parsed = parseFloat(rawValue); return isNaN(parsed) ? 0 : parsed }
    if (typeof rawValue === 'boolean') return rawValue ? 1 : 0
    return 0
  }
  return 0
}

const extractTimestamp = (point: unknown): number => {
  if (typeof point === 'object' && point !== null) {
    const p = point as Record<string, unknown>
    const ts = p.timestamp ?? p.time ?? p.t
    if (typeof ts === 'number') return ts > 10000000000 ? Math.floor(ts / 1000) : ts
  }
  return Math.floor(Date.now() / 1000)
}

// ============================================================================
// Telemetry fetching (from telemetryFetch.ts)
// ============================================================================

const telemetryInflight = new Map<string, Promise<{ data: number[]; raw?: unknown[]; success: boolean }>>()

export async function fetchHistoricalTelemetry(
  deviceId: string,
  metricId: string,
  timeRange: number = 1,
  limit: number = 50,
  aggregate: TelemetryAggregate = 'raw',
  includeRawPoints: boolean = false,
  bypassCache: boolean = false,
  timeWindow?: TimeWindowConfig,
  isImageSource: boolean = false,
): Promise<{ data: number[]; raw?: unknown[]; success: boolean }> {
  // Include a time bucket (minute-aligned) in the cache key so that stale data
  // from a previous time period is never served when fresh data is available.
  const timeBucket = Math.floor(Date.now() / 60000) // changes every 60s, aligned to cache TTL
  const cacheKey = `${deviceId}|${metricId}|${timeRange}|${limit}|${aggregate}|${timeWindow?.type ?? 'rel'}|${timeBucket}`
  const cached = telemetryCache.get(cacheKey)

  if (!bypassCache && cached) return { data: cached.data, raw: cached.raw, success: true }
  if (!bypassCache && telemetryInflight.has(cacheKey)) return telemetryInflight.get(cacheKey)!

  const fetchPromise = (async () => {
    try {
      const api = (await import('@/lib/api')).api
      const now = Date.now()

      // Calculate precise start/end using getTimeRange for all window types
      let startSec: number
      let endSec: number
      if (timeWindow) {
        const range = getTimeRange(timeWindow)
        startSec = range.start
        endSec = range.end
      } else {
        const effectiveTimeRange = timeRange > 0 ? timeRange : 5 / 60
        startSec = Math.floor((now - effectiveTimeRange * 60 * 60 * 1000) / 1000)
        endSec = Math.floor(now / 1000)
      }

      // Image/large-payload metrics: fetch exactly `limit` newest points.
      // Do NOT use bucketed mode for images — bucketing collapses multiple
      // images within the same time bucket into one, losing data.
      // Detection: explicit isImageSource flag OR metricId naming heuristic.
      const isImgByMetric = !!(metricId && (
        metricId.toLowerCase().includes('image') ||
        metricId.toLowerCase().includes('img') ||
        metricId.toLowerCase().includes('frame') ||
        metricId.toLowerCase().includes('snapshot') ||
        metricId.toLowerCase().includes('values.image')
      ))
      const isImgMetric = isImageSource || isImgByMetric
      // Images: no bucketing — just fetch `limit` newest points.
      // For non-image metrics with timeWindow, fetch up to 3000 for bucketed display.
      // For image metrics, always respect the caller's limit to avoid huge payloads (base64).
      const fetchLimit = isImgMetric
        ? limit
        : timeWindow
          ? 3000
          : Math.max(limit * 2, timeRange <= 1 ? 100 : Math.min(Math.ceil(timeRange * 17), 1000))

      // Use unified telemetry endpoint for transform/ai sources, device endpoint otherwise
      const isUnifiedSource = deviceId.startsWith('transform:') || deviceId.startsWith('ai:')
      let metricData: unknown[] | undefined

      const apiStart = performance.now()
      if (isUnifiedSource) {
        const response = await api.queryTelemetry(deviceId, metricId, startSec, endSec, fetchLimit, false)
        metricData = response?.data as unknown[] | undefined
      } else {
        const response = await api.getDeviceTelemetry(deviceId, metricId, startSec, endSec, fetchLimit, undefined, false)

        // Find metric data — exact match, then case-insensitive
        if (response?.data && typeof response.data === 'object') {
          const dataObj = response.data as Record<string, unknown>
          if (Array.isArray(dataObj[metricId])) {
            metricData = dataObj[metricId] as unknown[]
          } else {
            const lowerMetricId = metricId.toLowerCase()
            for (const key of Object.keys(dataObj)) {
              if (key.toLowerCase() === lowerMetricId && Array.isArray(dataObj[key])) {
                metricData = dataObj[key] as unknown[]
                break
              }
            }
          }
        }
      }

      const apiElapsed = performance.now() - apiStart
      if (apiElapsed > 2000) {
        console.warn(`[Telemetry] Slow query: ${deviceId}/${metricId} took ${Math.round(apiElapsed)}ms`)
      }

      if (Array.isArray(metricData) && metricData.length > 0) {
        const allValues = metricData.map(extractValue).filter((v: number) => typeof v === 'number' && !isNaN(v))

        let values: number[]
        let rawPoints: unknown[] | undefined

        if (aggregate === 'latest') {
          values = [allValues[0] ?? 0]
          if (includeRawPoints) {
            const pt = metricData[0]
            const rv = typeof pt === 'object' && pt !== null ? (pt as Record<string, unknown>).value ?? (pt as Record<string, unknown>).v ?? pt : pt
            rawPoints = [{ timestamp: extractTimestamp(pt), value: rv }]
          }
        } else if (aggregate === 'first') {
          values = [allValues[allValues.length - 1] ?? 0]
          if (includeRawPoints) {
            const pt = metricData[metricData.length - 1]
            const rv = typeof pt === 'object' && pt !== null ? (pt as Record<string, unknown>).value ?? (pt as Record<string, unknown>).v ?? pt : pt
            rawPoints = [{ timestamp: extractTimestamp(pt), value: rv }]
          }
        } else if (aggregate === 'avg') {
          const aggVal = allValues.length > 0 ? allValues.reduce((a, b) => a + b, 0) / allValues.length : 0
          values = [aggVal]
          if (includeRawPoints) { const latest = metricData[0]; rawPoints = [{ timestamp: extractTimestamp(latest), value: aggVal }] }
        } else if (aggregate === 'min') {
          const aggVal = allValues.reduce((a, b) => Math.min(a, b), Infinity)
          values = [aggVal]
          if (includeRawPoints) { const latest = metricData[0]; rawPoints = [{ timestamp: extractTimestamp(latest), value: aggVal }] }
        } else if (aggregate === 'max') {
          const aggVal = allValues.reduce((a, b) => Math.max(a, b), -Infinity)
          values = [aggVal]
          if (includeRawPoints) { const latest = metricData[0]; rawPoints = [{ timestamp: extractTimestamp(latest), value: aggVal }] }
        } else if (aggregate === 'sum') {
          const aggVal = allValues.reduce((a, b) => a + b, 0)
          values = [aggVal]
          if (includeRawPoints) { const latest = metricData[0]; rawPoints = [{ timestamp: extractTimestamp(latest), value: aggVal }] }
        } else if (aggregate === 'delta') {
          const aggVal = (allValues[0] ?? 0) - (allValues[allValues.length - 1] ?? 0)
          values = [aggVal]
          if (includeRawPoints) { const latest = metricData[0]; rawPoints = [{ timestamp: extractTimestamp(latest), value: aggVal }] }
        } else if (aggregate === 'count') {
          const aggVal = allValues.length
          values = [aggVal]
          if (includeRawPoints) { const latest = metricData[0]; rawPoints = [{ timestamp: extractTimestamp(latest), value: aggVal }] }
        } else if (aggregate === 'rate') {
          const firstTs = extractTimestamp(metricData[metricData.length - 1] ?? metricData[0])
          const lastTs = extractTimestamp(metricData[0])
          const timeDiff = lastTs - firstTs
          const aggVal = (timeDiff > 0 && allValues.length >= 2)
            ? ((allValues[0] ?? 0) - (allValues[allValues.length - 1] ?? 0)) / timeDiff
            : 0
          values = [aggVal]
          if (includeRawPoints) { const latest = metricData[0]; rawPoints = [{ timestamp: extractTimestamp(latest), value: aggVal }] }
        } else {
          // raw — sort once then cap, instead of O(n²) per-point insertion.
          // Backend returns newest-first; sort ascending (oldest-first) for display.
          const displayLimit = limit || 50
          const _preserveAll = includeRawPoints

          // Sort indices by timestamp ascending to avoid creating wrapper objects
          const indices = metricData.map((_, i) => i)
          indices.sort((a, b) => extractTimestamp(metricData[a]) - extractTimestamp(metricData[b]))
          const ascData = indices.map(i => metricData[i])

          // Apply limit (keep newest = last N)
          const trimmed = ascData.length > displayLimit
            ? ascData.slice(ascData.length - displayLimit)
            : ascData
          values = trimmed.map(extractValue).filter((v: number) => typeof v === 'number' && !isNaN(v))
          rawPoints = includeRawPoints ? trimmed.map((point) => {
            if (typeof point === 'number') return { timestamp: Math.floor(Date.now() / 1000), value: point }
            if (typeof point === 'object' && point !== null) {
              const p = point as Record<string, unknown>
              const ts = p.timestamp ?? p.time ?? p.t
              const timestamp = typeof ts === 'number' ? (ts > 10000000000 ? Math.floor(ts / 1000) : ts) : Math.floor(Date.now() / 1000)
              // For image sources, pre-normalize raw base64 to data URL once here
              // so toImageHistoryItems doesn't re-compute expensive normalization on every render
              if (isImgMetric) {
                const rawVal = p.value ?? p.v
                const normalized = normalizeImageValue(rawVal)
                if (normalized !== rawVal) {
                  return { ...p, timestamp, value: normalized }
                }
              }
              return { ...p, timestamp }
            }
            return { timestamp: Math.floor(Date.now() / 1000), value: point }
          }) : undefined
        }

        // For image sources, only cache the last 5 raw items to avoid
        // holding 30-50MB of base64 data in memory. Switching back to the
        // dashboard will instantly show the latest 5 images from cache while
        // the full history loads in the background.
        const rawToCache = isImgMetric && rawPoints && rawPoints.length > 5
          ? rawPoints.slice(-5)
          : rawPoints
        telemetryCache.set(cacheKey, { data: values, raw: rawToCache }, { cachedAt: Date.now() })
        return { data: values, raw: rawPoints, success: true }
      }

      return { data: [], success: false }
    } catch (error) {
      // Network errors are expected when offline — log as warning, not error
      if (isNetworkError(error)) {
        console.warn('[Fetch historical telemetry] Network error (offline?)')
      } else {
        logError(error, { operation: 'Fetch historical telemetry' })
      }
      return { data: [], success: false }
    }
  })()

  if (!bypassCache) {
    telemetryInflight.set(cacheKey, fetchPromise)
    fetchPromise.finally(() => telemetryInflight.delete(cacheKey))
  }

  return fetchPromise
}

// Expose cache for event processing (telemetry cache refresh scheduling)
export { telemetryCache }

// ============================================================================
// System stats fetching (from systemFetch.ts)
// ============================================================================

export async function fetchSystemStats(metric: string): Promise<{ data: unknown; success: boolean }> {
  const cacheKey = `system|${metric}`
  const cached = systemStatsCache.get(cacheKey)
  if (cached !== undefined) return { data: cached, success: true }

  try {
    const api = (await import('@/lib/api')).api
    const stats = await api.getSystemStats()
    if (!stats) return { data: null, success: false }

    let value: unknown = null
    switch (metric) {
      case 'uptime': value = stats.uptime; break
      case 'cpu_count': value = stats.cpu_count; break
      case 'total_memory': value = stats.total_memory / (1024 * 1024 * 1024); break
      case 'used_memory': value = stats.used_memory / (1024 * 1024 * 1024); break
      case 'free_memory': value = stats.free_memory / (1024 * 1024 * 1024); break
      case 'available_memory': value = stats.available_memory / (1024 * 1024 * 1024); break
      case 'memory_percent': value = stats.used_memory / stats.total_memory * 100; break
      case 'platform': value = stats.platform; break
      case 'arch': value = stats.arch; break
      case 'version': value = stats.version; break
    }

    systemStatsCache.set(cacheKey, value)
    return { data: value, success: true }
  } catch (error) {
    if (isNetworkError(error)) {
      console.warn('[Fetch system stats] Network error (offline?)')
    } else {
      logError(error, { operation: 'Fetch system stats' })
    }
    return { data: null, success: false }
  }
}

// ============================================================================
// Polling dispatch — generic fetch for non-WS sources
// ============================================================================

/**
 * Dispatch a polling fetch for a DataSource based on its source type.
 * Used by usePollingSource for all non-WS sources (system, rule, message, http, etc.)
 * Add new source types here — usePollingSource itself is generic.
 */
export async function pollDataSource(ds: DataSource): Promise<unknown> {
  const source = ds.source
  const field = getUnifiedField(ds)

  switch (source) {
    case 'system': {
      if (!field) return null
      const response = await fetchSystemStats(field)
      return response.data
    }
    // Future sources — add cases here:
    // case 'rule': { ... }
    // case 'message': { ... }
    // case 'http': { ... }
    default:
      throw new Error(`Unsupported polling source: ${source}`)
  }
}

// ============================================================================
// Batch device fetch (from batchFetch.ts)
// ============================================================================

export const fetchedDevices = new Set<string>()

const activeFetches = new Map<string, Promise<{ success: boolean; metricsCount: number }>>()
let pendingDeviceIds = new Set<string>()
let pendingResolvers = new Map<string, Array<(result: { success: boolean; metricsCount: number }) => void>>()
let batchScheduled = false

function applyBatchResults(
  results: Record<string, unknown>,
  deviceIds: string[],
  resolvers: Map<string, Array<(result: { success: boolean; metricsCount: number }) => void>>
) {
  // Use direct set() instead of updateDeviceMetric (which goes through BatchUpdater RAF)
  // to avoid an extra ~16ms delay before store subscribers are notified.
  const store = useStore.getState()
  store._applyCurrentValuesBatch(results, deviceIds)
  for (const id of deviceIds) {
    const entry = results[id] as { current_values?: Record<string, unknown> } | undefined
    let metricsCount = 0
    if (entry?.current_values && typeof entry.current_values === 'object') {
      metricsCount = Object.values(entry.current_values).filter(v => v !== null && v !== undefined).length
    }
    if (metricsCount > 0) {
      fetchedDevices.add(id)
      if (fetchedDevices.size > 200) { const first = fetchedDevices.values().next().value; if (first) fetchedDevices.delete(first) }
    }
    activeFetches.delete(id)
    resolvers.get(id)?.forEach(r => r({ success: metricsCount > 0, metricsCount }))
    resolvers.delete(id)
  }
}

async function flushBatch() {
  const ids = Array.from(pendingDeviceIds)
  const resolvers = pendingResolvers
  pendingDeviceIds = new Set()
  pendingResolvers = new Map()
  batchScheduled = false

  if (ids.length === 0) return

  try {
    const api = (await import('@/lib/api')).api
    const batchResult = await api.getDevicesCurrentBatch(ids)
    if (batchResult?.devices && typeof batchResult.devices === 'object') {
      applyBatchResults(batchResult.devices as Record<string, unknown>, ids, resolvers)
    } else {
      for (const id of ids) {
        activeFetches.delete(id)
        resolvers.get(id)?.forEach(r => r({ success: false, metricsCount: 0 }))
        resolvers.delete(id)
      }
    }
  } catch (error) {
    if (!isNetworkError(error)) {
      logError(error, { operation: 'Batch fetch devices, falling back to individual' })
    }
    // Fallback to individual fetches
    try {
      const api = (await import('@/lib/api')).api
      const CHUNK_SIZE = 5
      const individualResults: Array<PromiseSettledResult<{ id: string; success: boolean; metricsCount: number }>> = []
      // Collect all fetched device data for batch store write
      const fallbackResults: Record<string, unknown> = {}
      const fallbackIds: string[] = []
      for (let i = 0; i < ids.length; i += CHUNK_SIZE) {
        const chunk = ids.slice(i, i + CHUNK_SIZE)
        const chunkResults = await Promise.allSettled(chunk.map(async (id) => {
          try {
            const details = await api.getDeviceCurrent(id)
            let metricsCount = 0
            if (details?.metrics) {
              // Collect for batch apply instead of individual store.updateDeviceMetric
              fallbackResults[id] = { current_values: details.metrics }
              fallbackIds.push(id)
              metricsCount = Object.values(details.metrics).filter((v: any) => v?.value !== null && v?.value !== undefined).length
            }
            if (metricsCount > 0) fetchedDevices.add(id)
            return { id, success: metricsCount > 0, metricsCount }
          } catch { return { id, success: false, metricsCount: 0 } }
        }))
        individualResults.push(...chunkResults)
      }
      // Apply all collected results at once via direct set()
      if (fallbackIds.length > 0) {
        const store = useStore.getState()
        store._applyCurrentValuesBatch(fallbackResults, fallbackIds)
      }
      for (let i = 0; i < ids.length; i++) {
        const id = ids[i]
        const settled = individualResults[i]
        const result = settled?.status === 'fulfilled' ? settled.value : { success: false, metricsCount: 0 }
        activeFetches.delete(id)
        resolvers.get(id)?.forEach(r => r(result))
        resolvers.delete(id)
      }
    } catch (innerError) {
      logError(innerError, { operation: 'Individual device fetch fallback failed' })
      for (const id of ids) {
        activeFetches.delete(id)
        resolvers.get(id)?.forEach(r => r({ success: false, metricsCount: 0 }))
        resolvers.delete(id)
      }
    }
  }
}

export async function fetchDeviceTelemetry(deviceId: string): Promise<{ success: boolean; metricsCount: number }> {
  const existing = activeFetches.get(deviceId)
  if (existing) return existing

  const promise = new Promise<{ success: boolean; metricsCount: number }>((resolve) => {
    pendingResolvers.set(deviceId, [...(pendingResolvers.get(deviceId) ?? []), resolve])
  })

  activeFetches.set(deviceId, promise)
  pendingDeviceIds.add(deviceId)

  if (!batchScheduled) { batchScheduled = true; queueMicrotask(flushBatch) }

  return promise
}

export function hasActiveFetch(deviceId: string): boolean {
  return activeFetches.has(deviceId)
}
