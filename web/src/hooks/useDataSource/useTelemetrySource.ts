/**
 * useTelemetrySource — handles telemetry fetch + periodic refresh + retry.
 * Also handles devices loading watcher that defers telemetry fetch.
 */

import { useEffect, useRef } from 'react'
import type { DataSource } from '@/types/dashboard'
import { getUnifiedField, getEventDeviceId } from '@/types/dashboard'
import type { NeoMindStore } from '@/store'
import { useStore } from '@/store'
import { logError } from '@/lib/errors'
import { fetchHistoricalTelemetry } from './fetch'
import {
  isImageDataSource, 
} from './helpers'
import {
  getTs, getNewestTimestamp, extractPointsNewerThan, mergeLiveData,
  sortTelemetryResults,
} from './eventProcessors'

/**
 * Check if a data source truly represents image data (not just any raw source).
 * Uses metricId naming heuristic — more restrictive than isImageDataSource
 * which also matches any includeRawPoints/raw transform.
 */
function isActualImageSource(metricId: string | undefined): boolean {
  if (!metricId) return false
  const lower = metricId.toLowerCase()
  return lower.includes('image') || lower.includes('img') ||
         lower.includes('frame') || lower.includes('snapshot') ||
         lower.includes('photo') || lower.includes('capture') ||
         metricId.includes('values.image')
}

export interface TelemetrySourceState {
  setData: (value: unknown | ((prev: unknown) => unknown)) => void
  setDataRaw: (d: unknown) => void
  setLoading: (l: boolean) => void
  setError: (e: string | null) => void
  setLastUpdate: (ts: number | null) => void
  optionsRef: React.MutableRefObject<{
    enabled: boolean
    transform?: (data: unknown) => unknown
    fallback?: unknown
    preserveMultiple: boolean
  }>
  readDataFromStore: () => void
  /** Source-scoped loading adapters for useReducer state machine */
  sourceAdapters?: {
    startLoading: () => void
    finishLoading: () => void
    retryLoading: () => void
    failLoading: (error: string) => void
    forceFinishLoading: () => void
  }
}

// Maximum time (ms) the loading state is allowed to persist before being
// force-cleared.  Prevents skeleton screens from appearing stuck when the
// backend returns empty data or the retry loop runs too long.
const MAX_LOADING_DURATION = 3000

export function useTelemetrySource(
  telemetrySources: DataSource[],
  telemetryKey: string,
  enabled: boolean,
  hasTelemetrySource: boolean,
  relevantDeviceIds: Set<string>,
  wsConnected: boolean,
  state: TelemetrySourceState
): void {
  const initialTelemetryFetchDoneRef = useRef(false)
  const emptyRetryCountRef = useRef(0)
  const retryInProgressRef = useRef(false)
  const deferredByDevicesLoadingRef = useRef(false)
  const telemetryIntervalRef = useRef<ReturnType<typeof setInterval> | null>(null)
  const retryTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null)
  const fetchTimeoutRef = useRef<ReturnType<typeof setTimeout> | null>(null)
  const maxLoadingTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null)
  const prevTelemetryKeyRef = useRef('')
  const fetchGenerationRef = useRef(0)

  // ============================================================================
  // Devices loading watcher
  // ============================================================================

  useEffect(() => {
    if (relevantDeviceIds.size === 0 && !hasTelemetrySource) return

    let prevLoading = useStore.getState().devicesLoading
    const unsubscribe = useStore.subscribe((s: NeoMindStore) => {
      if (s.devicesLoading === prevLoading) return
      prevLoading = s.devicesLoading

      if (!s.devicesLoading && deferredByDevicesLoadingRef.current) {
        deferredByDevicesLoadingRef.current = false
        state.readDataFromStore()
        // readDataFromStore may return early for telemetry-only sources
        // without calling finishLoading — do it here to unblock loading.
        if (!retryInProgressRef.current) {
          if (state.sourceAdapters) state.sourceAdapters.finishLoading()
          else state.setLoading(false)
        }
      }
    })

    return () => unsubscribe()
  }, [relevantDeviceIds, hasTelemetrySource])

  // ============================================================================
  // Telemetry fetch effect
  // ============================================================================

  useEffect(() => {
    if (!hasTelemetrySource || !enabled) {
      if (telemetryIntervalRef.current) { clearInterval(telemetryIntervalRef.current); telemetryIntervalRef.current = null }
      return
    }

    const configChanged = prevTelemetryKeyRef.current !== telemetryKey
    if (configChanged && telemetryKey) {
      initialTelemetryFetchDoneRef.current = false
      emptyRetryCountRef.current = 0
      retryInProgressRef.current = false
      // Cache key already includes deviceId+metricId+timeRange+aggregate+timeBucket,
      // so different configs naturally miss; and 60s TTL bounds staleness.
      // Do NOT delete cache here — that would defeat cache reuse on dashboard switching.
    }
    prevTelemetryKeyRef.current = telemetryKey

    // Bump generation so stale in-flight fetches can be discarded
    const currentGeneration = ++fetchGenerationRef.current

    const fetchTelemetryData = async () => {
      const isInitialFetch = !initialTelemetryFetchDoneRef.current
      if (isInitialFetch) {
        if (state.sourceAdapters) state.sourceAdapters.startLoading()
        else state.setLoading(true)
      }
      state.setError(null)

      // Track timeout so it can be cleaned up on unmount
      if (fetchTimeoutRef.current) clearTimeout(fetchTimeoutRef.current)
      const timeoutPromise = new Promise<never>((_, reject) => {
        fetchTimeoutRef.current = setTimeout(() => {
          fetchTimeoutRef.current = null
          reject(new Error('Fetch timeout'))
        }, 10000)
      })

      try {
        const results = await Promise.race([
          Promise.all(
            telemetrySources.map(async (ds) => {
              const dsMetricId = getUnifiedField(ds)
              // getEventDeviceId returns the prefixed id ("transform:{id}", "ai:{id}")
              // for transform/ai sources, or the bare id for devices.
              // fetchHistoricalTelemetry uses the prefix to route to the unified
              // telemetry endpoint (api.queryTelemetry) instead of the device endpoint.
              const dsSourceId = getEventDeviceId(ds)
              if (!dsSourceId || !dsMetricId) return { data: [], raw: undefined }
              const includeRawPoints = ds.params?.includeRawPoints === true || ds.transform === 'raw'
              // Always prefer cache: it's 60s-bucket-aligned + 30s-TTL-protected.
              // Previous logic bypassed cache on (a) initial fetch and (b) raw/chart data,
              // which forced every dashboard switch and every chart into a cold fetch.
              // Charts and dashboard switching now reuse cached data instantly while
              // the periodic poll (or WS push) refreshes in the background.
              const bypassCache = false
              const isImg = isImageDataSource(ds)
              const actualTimeRange = ds.timeRange ?? (isImg ? 48 : 1)
              const actualLimit = ds.limit ?? (isImg ? 200 : 50)
              // Charts (includeRawPoints=true) must always fetch raw data to preserve
              // timestamps for time-series rendering. Value components (includeRawPoints=false)
              // use aggregateExt for single-value aggregation.
              const actualAggregate = includeRawPoints ? 'raw' : (ds.aggregateExt ?? 'raw')

              const response = await fetchHistoricalTelemetry(
                dsSourceId, dsMetricId, actualTimeRange, actualLimit, actualAggregate, includeRawPoints, bypassCache,
                ds.timeWindow,
                ds.params?.isImage === true || isActualImageSource(dsMetricId),
              )
              if (includeRawPoints && response.raw) return { data: response.data, raw: response.raw, success: response.success }
              return { data: response.success ? response.data : [], success: response.success }
            })
          ),
          timeoutPromise
        ]) as Array<{ data: unknown[]; raw?: unknown[]; success: boolean }>

        // Clear timeout — fetch succeeded before the deadline
        if (fetchTimeoutRef.current) { clearTimeout(fetchTimeoutRef.current); fetchTimeoutRef.current = null }
        // Discard stale results if config changed while fetching
        if (fetchGenerationRef.current !== currentGeneration) return

        let finalData: unknown
        const pm = state.optionsRef.current.preserveMultiple
        if (results.length > 1) {
          if (pm) {
            const hasRawData = results.some((r) => r.raw !== undefined)
            finalData = hasRawData ? results.map((r) => r.raw ?? []) : results.map((r) => r.data ?? [])
          } else {
            const hasRawData = results.some((r) => r.raw !== undefined)
            finalData = hasRawData ? results.flatMap((r) => r.raw ?? []) : results.map((r) => r.data ?? []).flat()
          }
        } else {
          const r = results[0]
          finalData = (r?.raw ?? r?.data) ?? []
        }

        // Sort and dedup
        finalData = sortTelemetryResults(finalData, telemetrySources, pm)

        state.setData((prevData: unknown) => {
          // No previous data — just set fetched data
          if (prevData == null) return finalData

          // Don't merge when fetched data is empty — let empty handling work
          const fetchedArr = Array.isArray(finalData) ? finalData : []
          if (fetchedArr.length === 0) {
            if (prevData == null || (Array.isArray(prevData) && (prevData as unknown[]).length === 0)) return finalData
            // Periodic refresh: preserve existing data instead of blanking charts
            return prevData
          }

          // Only merge live points that are strictly newer than the newest fetched point
          const newestFetchedTs = getNewestTimestamp(finalData, getTs)
          if (newestFetchedTs === 0) return finalData

          const sixtySecondsAgo = Math.floor(Date.now() / 1000) - 60
          const cutoffTs = Math.max(newestFetchedTs, sixtySecondsAgo)
          const livePoints = extractPointsNewerThan(prevData, cutoffTs, getTs, pm, telemetrySources.length)

          if (livePoints.length === 0) return finalData

          // Merge live points into fetched data
          return mergeLiveData(finalData, livePoints, getTs, pm, telemetrySources)
        })
        state.setLastUpdate(Date.now())
        initialTelemetryFetchDoneRef.current = true

        // Empty result — quick retry then accept empty state.
        // Only retry during the initial fetch cycle; subsequent polling refreshes
        // silently accept empty results to avoid perpetual skeleton state.
        const isEmpty = Array.isArray(finalData) ? finalData.length === 0 : finalData == null
        if (isEmpty && isInitialFetch) {
          const { devicesLoading } = useStore.getState()
          if (devicesLoading) {
            // Defer — keep loading, wait for devices to finish loading
            deferredByDevicesLoadingRef.current = true
            initialTelemetryFetchDoneRef.current = false
          } else {
            emptyRetryCountRef.current += 1
            // One quick retry (500ms) then give up — polling will pick up
            // data when it eventually arrives. Extended retry loops cause
            // long skeleton screens that feel broken to the user.
            const maxRetries = 1
            if (emptyRetryCountRef.current <= maxRetries) {
              const delay = 500
              retryInProgressRef.current = true
              // NOTE: Do NOT call startLoading/retryLoading here — the counter is already
              // incremented from the initial startLoading() above. We keep loading=true
              // by simply not calling finishLoading() in the finally block.
              if (retryTimerRef.current) clearTimeout(retryTimerRef.current)
              retryTimerRef.current = setTimeout(() => {
                retryTimerRef.current = null
                if (fetchGenerationRef.current !== currentGeneration) return
                fetchTelemetryData()
              }, delay)
            } else {
              // Retries exhausted — stop retrying, finally block will finishLoading
              retryInProgressRef.current = false
            }
          }
        } else {
          emptyRetryCountRef.current = 0
          retryInProgressRef.current = false
        }
      } catch (err) {
        // Discard stale error if config changed while fetching
        if (fetchGenerationRef.current !== currentGeneration) return
        logError(err, { operation: 'Fetch telemetry data' })
        state.setError(err instanceof Error ? err.message : 'Failed to fetch telemetry')
        // Preserve previous data on error
        if (initialTelemetryFetchDoneRef.current) {
          const { transform: transformFn } = state.optionsRef.current
          state.setData((prev: unknown) => {
            if (prev == null || (Array.isArray(prev) && (prev as unknown[]).length === 0)) return (transformFn ? transformFn([]) : [])
            return prev
          })
        } else {
          state.setDataRaw([])
        }
        initialTelemetryFetchDoneRef.current = true
      } finally {
        // Only clear loading when not retrying for empty results and not deferred
        if (fetchGenerationRef.current === currentGeneration && !deferredByDevicesLoadingRef.current && !retryInProgressRef.current) {
          if (state.sourceAdapters) state.sourceAdapters.finishLoading()
          else state.setLoading(false)
        }
      }
    }

    if (telemetryIntervalRef.current) { clearInterval(telemetryIntervalRef.current); telemetryIntervalRef.current = null }
    fetchTelemetryData()

    // Always start polling as a safety net. WS delivers real-time updates,
    // but if a device hasn't sent data yet or data was missed, polling fills the gap.
    // Use a longer interval when WS is connected (just a backup) vs disconnected (primary source).
    const refreshIntervals = telemetrySources.map((ds) => ds.refresh).filter(Boolean) as number[]
    const baseRefresh = refreshIntervals.length > 0 ? refreshIntervals.reduce((a, b) => Math.min(a, b), Infinity) : 30
    const pollingInterval = wsConnected ? baseRefresh * 2 : baseRefresh
    telemetryIntervalRef.current = setInterval(fetchTelemetryData, pollingInterval * 1000)

    // Hard deadline: force-clear loading after MAX_LOADING_DURATION regardless
    // of retry state, deferred state, or slow API responses.  This guarantees
    // the user never sees a stuck skeleton screen for more than a few seconds.
    if (maxLoadingTimerRef.current) clearTimeout(maxLoadingTimerRef.current)
    maxLoadingTimerRef.current = setTimeout(() => {
      maxLoadingTimerRef.current = null
      if (fetchGenerationRef.current === currentGeneration) {
        retryInProgressRef.current = false
        deferredByDevicesLoadingRef.current = false
        if (state.sourceAdapters) state.sourceAdapters.forceFinishLoading()
        else state.setLoading(false)
      }
    }, MAX_LOADING_DURATION)

    return () => {
      if (telemetryIntervalRef.current) { clearInterval(telemetryIntervalRef.current); telemetryIntervalRef.current = null }
      if (retryTimerRef.current) { clearTimeout(retryTimerRef.current); retryTimerRef.current = null }
      if (fetchTimeoutRef.current) { clearTimeout(fetchTimeoutRef.current); fetchTimeoutRef.current = null }
      if (maxLoadingTimerRef.current) { clearTimeout(maxLoadingTimerRef.current); maxLoadingTimerRef.current = null }
    }
  }, [telemetryKey, enabled, wsConnected])
}
