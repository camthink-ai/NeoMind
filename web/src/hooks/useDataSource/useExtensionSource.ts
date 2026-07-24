/**
 * useExtensionSource — handles extension data fetch + extension WebSocket events.
 */

import { useEffect, useRef, useMemo } from 'react'
import type { DataSource } from '@/types/dashboard'
import { getUnifiedId, getUnifiedField } from '@/types/dashboard'
import { logError } from '@/lib/errors'
import { useEvents } from '@/hooks/useEvents'
import { getTimeRange, getEffectiveTimeWindow } from '@/lib/telemetryTransform'
import { extensionDataCache } from './fetch'
import { normalizeOutputName, getTs } from './eventProcessors'

export interface ExtensionSourceState {
  setData: (value: unknown | ((prev: unknown) => unknown)) => void
  setDataRaw: (d: unknown) => void
  setLoading: (l: boolean) => void
  setError: (e: string | null) => void
  setLastUpdate: (ts: number | null) => void
  dataSourcesRef: React.MutableRefObject<DataSource[]>
  optionsRef: React.MutableRefObject<{
    enabled: boolean
    transform?: (data: unknown) => unknown
    fallback?: unknown
    preserveMultiple: boolean
  }>
  sourceAdapters?: {
    startLoading: () => void
    finishLoading: () => void
    failLoading: (error: string) => void
  }
}

export function useExtensionSource(
  extensionSources: DataSource[],
  extensionKey: string,
  enabled: boolean,
  dataSourceKey: string,
  relevantExtensionIds: Set<string>,
  state: ExtensionSourceState
): void {
  const extInitialDoneRef = useRef(false)
  const prevExtKeyRef = useRef('')
  const extensionIntervalRef = useRef<ReturnType<typeof setInterval> | null>(null)

  // Event processing refs
  const processedExtEventsRef = useRef<Set<string>>(new Set())
  const lastProcessedExtEventIdRef = useRef<string | null>(null)

  // ============================================================================
  // Extension WebSocket (must be before fetch effect so extWsConnected is available)
  // ============================================================================

  const { events: extensionEvents, isConnected: extWsConnected } = useEvents({
    enabled: enabled && extensionSources.length > 0,
    category: 'extension',
    onConnected: () => {
      processedExtEventsRef.current.clear()
      lastProcessedExtEventIdRef.current = null
      // Invalidate caches so next fetch cycle gets fresh data
      extensionSources.forEach((ds) => {
        const extId = getUnifiedId(ds)
        const metric = getUnifiedField(ds) ?? ''
        if (extId && metric) {
          const cacheKey = `${extId}|${metric}|`
          extensionDataCache.deleteWhere((_, key) => key.startsWith(cacheKey))
        }
      })
      extInitialDoneRef.current = false
    },
  })

  // ============================================================================
  // Extension fetch
  // ============================================================================

  useEffect(() => {
    if (extensionSources.length === 0 || !enabled) {
      if (extensionIntervalRef.current) { clearInterval(extensionIntervalRef.current); extensionIntervalRef.current = null }
      return
    }

    // Reset when extension config changes
    if (prevExtKeyRef.current !== extensionKey) {
      extInitialDoneRef.current = false
      prevExtKeyRef.current = extensionKey
      // Clear processed events to avoid stale dedup state
      processedExtEventsRef.current.clear()
      lastProcessedExtEventIdRef.current = null
    }

    const fetchExtensionData = async () => {
      if (!extInitialDoneRef.current) {
        if (state.sourceAdapters) state.sourceAdapters.startLoading()
        else state.setLoading(true)
      }
      state.setError(null)

      try {
        const { transform: transformFn } = state.optionsRef.current
        const api = (await import('@/lib/api')).api
        const results = await Promise.all(
          extensionSources.map(async (ds) => {
            const extensionId = getUnifiedId(ds)
            const metric = getUnifiedField(ds) ?? ''
            if (!extensionId || !metric) return { data: null }

            // Compute time range from dataSource's timeWindow, falling back to timeRange
            // (also reused as cache-key component below — avoids duplicate getEffectiveTimeWindow call)
            const effectiveTimeWindow = ds.timeWindow ?? (
              ds.timeRange != null ? getEffectiveTimeWindow(ds) : undefined
            )
            let startMs: number
            let endMs: number
            if (effectiveTimeWindow) {
              const range = getTimeRange(effectiveTimeWindow)
              startMs = range.start * 1000
              endMs = range.end * 1000
            } else {
              const hours = ds.timeRange ?? 1
              endMs = Date.now()
              startMs = endMs - hours * 60 * 60 * 1000
            }
            const userLimit = ds.limit ?? 100
            // When user explicitly set a timeWindow, keep all returned points to cover
            // the full time range. Only truncate for default/relative queries.
            const hasExplicitTimeWindow = !!effectiveTimeWindow

            // Cache key includes time bucket so stale data doesn't persist
            const timeBucket = Math.floor(Date.now() / 60000)
            const extCacheKey = `${extensionId}|${metric}|${effectiveTimeWindow?.type ?? 'rel'}|${userLimit}|${timeBucket}`
            const extCached = extensionDataCache.get(extCacheKey)
            if (extCached !== undefined) return { data: extCached, success: true }

            // V2 data source (format: command:field)
            const isV2 = metric.includes(':')
            const parts = metric.split(':')

            try {
              if (isV2 && parts.length >= 2) {
                const command = parts[0]
                const field = parts[1]

                // When a timeWindow is configured, the user expects historical data.
                // Skip executeExtensionCommand (which ignores time range) and go
                // straight to queryData so the time range is respected.
                const needsTimeRange = !!effectiveTimeWindow

                if (command !== 'produce' && !needsTimeRange) {
                  try {
                    // skipErrorToast: this is a background pull for dashboard data; a command
                    // with unmet required params (e.g. get_weather w/o a default city) should
                    // not spam the user — fall through to queryData silently (catch below).
                    const result = await api.executeExtensionCommand(extensionId, command, {}, { skipErrorToast: true })
                    const resultData = (result as Record<string, unknown>).result ?? result

                    if (field === 'result') return { data: resultData, success: true }
                    if (typeof resultData === 'object' && resultData !== null) {
                      const fieldValue = (resultData as Record<string, unknown>)[field]
                      return { data: fieldValue ?? resultData, success: true }
                    }
                    return { data: resultData, success: true }
                  } catch {
                    // Don't pass limit — backend returns oldest-first, so a small limit
                    // would only return old data. Fetch all and truncate client-side.
                    const result = await api.queryData({
                      extension_id: extensionId, command, field,
                      start_time: startMs, end_time: endMs,
                    })
                    if (result?.data_points?.length > 0) {
                      const points = result.data_points
                      // Keep all points for time-window queries, truncate only for defaults
                      const truncated = (!hasExplicitTimeWindow && points.length > userLimit) ? points.slice(-userLimit) : points
                      return { data: truncated, success: true }
                    }
                    return { data: null, success: false }
                  }
                }

                // produce:* format OR timeWindow configured — use queryData with time range
                const result = await api.queryData({
                  extension_id: extensionId, command, field,
                  start_time: startMs, end_time: endMs,
                })

                if (result?.data_points?.length > 0) {
                  const points = result.data_points
                  // Keep all points for time-window queries to cover the full range.
                  // Only apply userLimit for default/no-window queries.
                  const truncated = (!hasExplicitTimeWindow && points.length > userLimit) ? points.slice(-userLimit) : points
                  return { data: truncated, success: true }
                }

                // queryData returned nothing — try executeExtensionCommand as fallback
                // (some extensions don't store in time-series DB but respond to commands)
                if (command !== 'produce') {
                  try {
                    const cmdResult = await api.executeExtensionCommand(extensionId, command, {}, { skipErrorToast: true })
                    const resultData = (cmdResult as Record<string, unknown>).result ?? cmdResult
                    if (field === 'result') return { data: resultData, success: true }
                    if (typeof resultData === 'object' && resultData !== null) {
                      const fieldValue = (resultData as Record<string, unknown>)[field]
                      return { data: fieldValue ?? resultData, success: true }
                    }
                    return { data: resultData, success: true }
                  } catch {
                    // Both paths failed
                  }
                }

                return { data: null, success: false }
              } else {
                return { data: null, success: false }
              }
            } catch {
              return { data: null, success: false }
            }
          })
        )

        // Cache successful results — reuse effectiveTimeWindow computed during fetch
        // instead of calling getEffectiveTimeWindow again per data source
        const cacheTimeBucket = Math.floor(Date.now() / 60000)
        extensionSources.forEach((ds, i) => {
          const extId = getUnifiedId(ds)
          const metric = getUnifiedField(ds) ?? ''
          if (extId && metric && results[i]?.success) {
            const tw = ds.timeWindow ?? (ds.timeRange != null ? getEffectiveTimeWindow(ds) : undefined)
            const key = `${extId}|${metric}|${tw?.type ?? 'rel'}|${ds.limit ?? 100}|${cacheTimeBucket}`
            extensionDataCache.set(key, results[i].data)
          }
        })

        let finalData: unknown
        if (results.length > 1) finalData = results.map((r) => r.data)
        else finalData = results[0]?.data ?? null

        // NOTE: Do NOT wrap scalar values into [{ timestamp: now, value }] before
        // the merge — using `now` as timestamp makes the merge think fetched data
        // is the newest, causing it to discard all accumulated WS live points.
        // Scalar wrapping is handled inside the merge instead.

        // Merge: preserve live WebSocket points that are newer than fetched data
        // (instead of blindly replacing, which causes WS data to flash and disappear)

        const isScalar = finalData !== null && finalData !== undefined && !Array.isArray(finalData)

        state.setData((prevData: unknown) => {
          // No previous data — wrap scalar if needed, or use fetched data directly
          if (prevData == null) {
            if (isScalar) {
              const now = Math.floor(Date.now() / 1000)
              const wrapped = [{ timestamp: now, time: now, value: finalData }]
              return transformFn ? transformFn(wrapped) : wrapped
            }
            return transformFn ? transformFn(finalData) : finalData
          }

          // Scalar fetched data + existing WS data → preserve WS history
          // (WS events already contain the latest value; the scalar fetch adds nothing)
          if (isScalar) {
            if (Array.isArray(prevData) && (prevData as unknown[]).length > 0) {
              return prevData
            }
            // prevData exists but is not an array (edge case) — wrap scalar
            const now = Math.floor(Date.now() / 1000)
            const wrapped = [{ timestamp: now, time: now, value: finalData }]
            return transformFn ? transformFn(wrapped) : wrapped
          }

          // Fetched data is empty — preserve existing
          const fetchedArr = Array.isArray(finalData) ? finalData : []
          if (fetchedArr.length === 0) {
            if (prevData == null || (Array.isArray(prevData) && (prevData as unknown[]).length === 0)) {
              return transformFn ? transformFn(finalData) : finalData
            }
            return prevData
          }

          // Find the newest timestamp in fetched data
          let newestFetchedTs = 0
          for (const p of fetchedArr) {
            const ts = getTs(p)
            if (ts > newestFetchedTs) newestFetchedTs = ts
          }
          if (newestFetchedTs === 0) return transformFn ? transformFn(finalData) : finalData

          // Extract live points from prevData that are strictly newer
          const prevArr = Array.isArray(prevData) ? prevData as unknown[] : []
          const sixtySecondsAgo = Math.floor(Date.now() / 1000) - 60
          const cutoffTs = Math.max(newestFetchedTs, sixtySecondsAgo)
          const livePoints = prevArr.filter(p => getTs(p) > cutoffTs)
          if (livePoints.length === 0) return transformFn ? transformFn(finalData) : finalData

          // Merge live points into fetched data
          const merged = [...fetchedArr, ...livePoints]
          // Sort ascending by timestamp and dedup using composite key (ts + value)
          // to preserve multiple values within the same second
          merged.sort((a, b) => getTs(a) - getTs(b))
          const seen = new Set<string>()
          const deduped = merged.filter(p => {
            const ts = getTs(p)
            const val = (p as Record<string, unknown>).value
            // Lightweight fingerprint: avoid JSON.stringify on large base64 payloads
            let valKey: string
            if (typeof val === 'string' && val.length > 200) {
              valKey = `${val.length}:${val.slice(0, 64)}:${val.slice(-64)}`
            } else if (typeof val === 'object' && val !== null) {
              // For objects, use a shallow key from first-level values
              const keys = Object.keys(val as Record<string, unknown>).sort()
              valKey = keys.map(k => {
                const v = (val as Record<string, unknown>)[k]
                return typeof v === 'string' && v.length > 200
                  ? `${k}=${v.length}:${v.slice(0, 32)}:${v.slice(-32)}`
                  : `${k}=${v}`
              }).join(',')
            } else {
              valKey = String(val)
            }
            const key = `${ts}:${valKey}`
            if (seen.has(key)) return false
            seen.add(key)
            return true
          })

          return transformFn ? transformFn(deduped) : deduped
        })
        state.setLastUpdate(Date.now())
        extInitialDoneRef.current = true
      } catch (err) {
        logError(err, { operation: 'Fetch extension data' })
        state.setError(err instanceof Error ? err.message : 'Failed to fetch extension data')
        extInitialDoneRef.current = true
      } finally {
        if (state.sourceAdapters) state.sourceAdapters.finishLoading()
        else state.setLoading(false)
      }
    }

    if (extensionIntervalRef.current) { clearInterval(extensionIntervalRef.current); extensionIntervalRef.current = null }
    fetchExtensionData()

    // Only start polling when WS is disconnected (fallback mode)
    if (!extWsConnected) {
      const refreshIntervals = extensionSources.map((ds) => ds.refresh).filter(Boolean) as number[]
      const minRefresh = refreshIntervals.length > 0 ? refreshIntervals.reduce((a, b) => Math.min(a, b), Infinity) : 30 // Default 30s fallback
      extensionIntervalRef.current = setInterval(fetchExtensionData, minRefresh * 1000)
    }

    return () => { if (extensionIntervalRef.current) { clearInterval(extensionIntervalRef.current); extensionIntervalRef.current = null } }
  }, [extensionKey, enabled, extWsConnected])

  // ============================================================================
  // Extension WebSocket event processing
  // ============================================================================

  const extensionEventsKey = useMemo(() => {
    if (extensionEvents.length === 0) return 'empty'
    const lastEvent = extensionEvents[extensionEvents.length - 1]
    return `ext-events-${extensionEvents.length}-${lastEvent?.id || 'unknown'}`
  }, [extensionEvents])

  useEffect(() => {
    if (extensionSources.length === 0 || !enabled || extensionEvents.length === 0) return

    let extStartIndex = 0
    const lastProcessedExtId = lastProcessedExtEventIdRef.current
    if (lastProcessedExtId) {
      // Backward scan — cache-friendly, stops at first match
      let found = -1
      for (let i = extensionEvents.length - 1; i >= 0; i--) {
        if (extensionEvents[i].id === lastProcessedExtId) { found = i; break }
      }
      if (found !== -1) extStartIndex = found + 1
      else { extStartIndex = 0; const entries = Array.from(processedExtEventsRef.current); processedExtEventsRef.current = new Set(entries.slice(-50)) }
    }
    if (extStartIndex > extensionEvents.length) { extStartIndex = 0; processedExtEventsRef.current.clear() }

    const newEvents = extensionEvents.slice(extStartIndex)
    if (newEvents.length === 0) return

    // Use extensionSources directly instead of filtering dataSourcesRef each time
    const extDataSources = extensionSources
    if (extDataSources.length === 0) return

    // Build extensionId → DataSource[] map for O(1) lookup per event
    const extByDeviceId = new Map<string, DataSource[]>()
    for (const ds of extDataSources) {
      const extId = getUnifiedId(ds)
      if (!extId) continue
      const existing = extByDeviceId.get(extId)
      if (existing) existing.push(ds)
      else extByDeviceId.set(extId, [ds])
    }

    let lastProcessedExtIdInBatch: string | null = null

    for (const latestEvent of newEvents) {
      const eventData = (latestEvent as any).data || latestEvent
      const eventType = (latestEvent as any).type

      if (eventType !== 'ExtensionOutput') continue

      const eventExtensionId = eventData.extension_id as string
      const eventOutputName = eventData.output_name as string

      // Deterministic event ID using event content to properly deduplicate
      // Avoid JSON.stringify on large objects — use lightweight fingerprint
      const v = eventData.value
      let valueKey: string
      if (typeof v === 'string') {
        valueKey = v.length > 200 ? `${v.length}:${v.slice(0, 64)}:${v.slice(-64)}` : v
      } else if (typeof v === 'number' || typeof v === 'boolean') {
        valueKey = String(v)
      } else if (v !== null && typeof v === 'object') {
        const obj = v as Record<string, unknown>
        const keys = Object.keys(obj)
        valueKey = `obj:${keys.length}:${keys.slice(0, 5).join(',')}:${String(obj[keys[0]]).slice(0, 40)}`
      } else {
        valueKey = ''
      }
      const uniqueEventId = latestEvent.id || `${eventType}_${eventExtensionId || ''}_${eventOutputName || ''}_${eventData.timestamp || ''}_${valueKey}`
      if (processedExtEventsRef.current.has(uniqueEventId)) continue
      processedExtEventsRef.current.add(uniqueEventId)
      lastProcessedExtIdInBatch = uniqueEventId

      if (processedExtEventsRef.current.size > 100) {
        const entries = Array.from(processedExtEventsRef.current)
        processedExtEventsRef.current = new Set(entries.slice(-50))
      }

      if (!relevantExtensionIds.has(eventExtensionId)) continue

      const normalizedOutput = normalizeOutputName(eventOutputName)

      // O(1) lookup by extensionId, then filter by metric name
      const candidates = extByDeviceId.get(eventExtensionId)
      const matchingSources = candidates
        ? candidates.filter((ds) => {
            const dsField = getUnifiedField(ds) ?? ''
            if (!dsField) return false
            const parts = dsField.split(':')
            const metricName = parts.length > 1 ? parts[1] : parts[0]
            return metricName === normalizedOutput || metricName === eventOutputName
          })
        : []

      if (matchingSources.length > 0) {
        const { transform: transformFn, preserveMultiple: pm } = state.optionsRef.current
        const eventValue = eventData.value
        const now = Math.floor(Date.now() / 1000)
        const newPoint = { timestamp: now, time: now, value: eventValue }
        // Use setData(prev => ...) to avoid stale dataRef race condition
        state.setData((prevData: unknown) => {
          const currentData = prevData as unknown
          let newData: unknown

          if (pm && extDataSources.length > 1 && Array.isArray(currentData)) {
            const nested = (currentData as unknown[][]).map((arr, i) => {
              const ds = extDataSources[i]
              if (!ds) return arr
              const parts = (getUnifiedField(ds) ?? '').split(':')
              const metricName = parts.length > 1 ? parts[1] : parts[0]
              if (getUnifiedId(ds) === eventExtensionId && (metricName === normalizedOutput || metricName === eventOutputName)) {
                // Append new point (chronological order: oldest→newest, left→right)
                const maxLimit = ds.limit ?? 100
                if (!Array.isArray(arr) || arr.length === 0) return [newPoint]
                if (arr.length >= maxLimit) return [...arr.slice(arr.length - maxLimit + 1), newPoint]
                return [...arr, newPoint]
              }
              return arr
            })
            newData = nested
          } else if (Array.isArray(currentData)) {
            const maxLimit = (matchingSources[0] as any)?.limit ?? 100
            // Append new point (chronological order: oldest→newest, left→right)
            if (currentData.length >= maxLimit) newData = [...currentData.slice(currentData.length - maxLimit + 1), newPoint]
            else newData = [...currentData, newPoint]
          } else {
            newData = [newPoint]
          }

          return (transformFn ? transformFn(newData) : newData)
        })

        // Update cache with the merged live data so next poll doesn't overwrite with stale data
        matchingSources.forEach((ds) => {
          const extId = getUnifiedId(ds)
          const metric = getUnifiedField(ds) ?? ''
          if (extId && metric) {
            const cacheKey = `${extId}|${metric}|`
            extensionDataCache.deleteWhere((_, key) => key.startsWith(cacheKey))
          }
        })

        state.setLastUpdate(Date.now())
      }
    }

    if (lastProcessedExtIdInBatch) lastProcessedExtEventIdRef.current = lastProcessedExtIdInBatch
  }, [enabled, dataSourceKey, extensionEventsKey])
}
