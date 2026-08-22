/**
 * useDataSource — unified data binding for dashboard components.
 *
 * Thin router that delegates to focused sub-hooks:
 * - useStoreSource: device/metric/command/device-info + device WS
 * - useTelemetrySource: telemetry fetch + periodic refresh
 * - usePollingSource: system stats, rule lists, message lists, etc.
 * - useExtensionSource: extension fetch + extension WS
 */

import { useState, useCallback, useRef, useMemo } from 'react'
import type { DataSourceOrList, DataSource } from '@/types/dashboard'
import { normalizeDataSource, getUnifiedId, getUnifiedField, getUnifiedMode, getUnifiedSource, getEventDeviceId } from '@/types/dashboard'
import { toNumberArray } from '@/design-system/utils/format'
import { createStableKey } from '@/lib/stable-key'
import { isImageDataSource } from './helpers'
import {
  fetchHistoricalTelemetry,
  clearGlobalCacheIntervals,
} from './fetch'
import { usePollingSource } from './usePollingSource'
import { useExtensionSource } from './useExtensionSource'
import { useTelemetrySource } from './useTelemetrySource'
import { useStoreSource } from './useStoreSource'
import { parseExprRefs, refParts } from '@/lib/expressionSource'

// Re-export for backward compatibility
export { fetchHistoricalTelemetry, clearGlobalCacheIntervals }

// ============================================================================
// Types
// ============================================================================

export interface UseDataSourceResult<T = unknown> {
  data: T | null
  loading: boolean
  error: string | null
  lastUpdate: number | null
  sources: DataSource[]
  sendCommand?: (value?: unknown) => Promise<boolean>
  sending?: boolean
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
    preserveMultiple?: boolean
    sourceTransform?: (ds: DataSource) => DataSource | undefined
  }
): UseDataSourceResult<T> {
  const { enabled = true, transform, fallback, preserveMultiple = false, sourceTransform } = options ?? {}

  const hasDataSourceValue = dataSource !== undefined &&
                             dataSource !== null &&
                             (Array.isArray(dataSource) ? dataSource.length > 0 : true)

  // ============================================================================
  // A. State — flat useState instead of useReducer
  // ============================================================================

  const [data, setDataInternal] = useState<T | null>(fallback ?? null)
  const [error, setErrorInternal] = useState<string | null>(null)
  const [sending, setSending] = useState(false)
  const [lastUpdate, setLastUpdateInternal] = useState<number | null>(null)

  // Loading tracked via ref counter: loading = counter > 0
  const loadingRef = useRef(hasDataSourceValue && enabled ? 1 : 0)
  const [loading, setLoading] = useState(loadingRef.current > 0)

  // Track current data key for reset detection
  const currentKey = useMemo(() => createStableKey(dataSource), [dataSource])
  const prevKeyRef = useRef(currentKey)
  const initialRef = useRef(true)

  // Stable callbacks
  const setData = useCallback((value: T | ((prev: T | null) => T | null)) => {
    setDataInternal(prev => typeof value === 'function' ? (value as (p: T | null) => T | null)(prev) : value)
  }, [])

  const setDataRaw = useCallback((d: unknown) => {
    setDataInternal(d as T)
  }, [])

  const setError = useCallback((e: string | null) => {
    setErrorInternal(e)
  }, [])

  const setLastUpdate = useCallback((ts: number | null) => {
    if (ts !== null) setLastUpdateInternal(ts)
  }, [])

  const startLoading = useCallback(() => {
    loadingRef.current++
    if (loadingRef.current === 1) setLoading(true)
  }, [])

  const finishLoading = useCallback(() => {
    loadingRef.current = Math.max(0, loadingRef.current - 1)
    if (loadingRef.current === 0) setLoading(false)
    setLastUpdateInternal(Date.now())
  }, [])

  const forceFinishLoading = useCallback(() => {
    loadingRef.current = 0
    setLoading(false)
    setLastUpdateInternal(Date.now())
  }, [])

  const setErrorLoading = useCallback((err: string) => {
    loadingRef.current = Math.max(0, loadingRef.current - 1)
    if (loadingRef.current === 0) setLoading(false)
    setErrorInternal(err)
  }, [])

  // ============================================================================
  // A2. Reset when dataSource changes
  // ============================================================================

  if (prevKeyRef.current !== currentKey) {
    prevKeyRef.current = currentKey
    loadingRef.current = 1
    setLoading(true)
    setErrorInternal(null)
    setDataInternal(fallback ?? null)
  }
  // Skip initial loading clear on first render when no data source
  if (initialRef.current) {
    initialRef.current = false
    if (!hasDataSourceValue || !enabled) {
      loadingRef.current = 0
    }
  }

  // ============================================================================
  // B. Data source normalization + computed values
  // ============================================================================

  const dataSources = useMemo(() => {
    const normalized = dataSource ? normalizeDataSource(dataSource) : []
    if (sourceTransform) return normalized.map(sourceTransform).filter(Boolean) as DataSource[]
    return normalized
  }, [dataSource, sourceTransform])

  const optionsRef = useRef({ enabled, transform, fallback, preserveMultiple })
  optionsRef.current = { enabled, transform, fallback, preserveMultiple }
  const dataSourcesRef = useRef(dataSources)
  dataSourcesRef.current = dataSources

  const commandSource = useMemo(() => dataSources.find(ds => ds.mode === 'command'), [dataSources])
  const hasCommandSource = commandSource !== undefined

  // Single-pass categorization — extracts categories + ID sets in one loop
  const {
    telemetrySources, pollingSources, extensionSources, needsWebSocket,
    relevantDeviceIds, deviceInfoIds,
  } = useMemo(() => {
    const tel: DataSource[] = []
    const poll: DataSource[] = []
    const ext: DataSource[] = []
    const deviceIds = new Set<string>()
    const infoIds = new Set<string>()
    let needsWs = false
    for (const ds of dataSources) {
      const mode = getUnifiedMode(ds)
      const source = getUnifiedSource(ds)
      // Any live mode needs the device WebSocket for real-time updates. This
      // MUST run before the `continue` statements below — timeseries sources
      // `continue` after pushing to `tel`, which previously skipped this line
      // and silently disabled WS for ALL telemetry components (image/chart
      // history never updated in real time, only via the 30s poll).
      if (mode === 'latest' || mode === 'command' || mode === 'info' || mode === 'timeseries') needsWs = true
      // Collect device IDs for WS event filtering.
      // Transform/AI sources use prefixed IDs ("transform:{id}") so WS events
      // from the backend (which publish with storage_device_id namespace) match.
      if (source === 'device' || source === 'transform' || source === 'ai') {
        const id = getEventDeviceId(ds)
        if (id) deviceIds.add(id)
        if (mode === 'info') infoIds.add(id!)
      }
      if (source === 'expression') {
        // Every referenced device metric feeds the expression — subscribe to
        // all of them so store telemetry changes re-trigger the store read.
        const refs = parseExprRefs(ds.expr ?? '')
        for (const r of refs) {
          const parts = refParts(r)
          if (parts) deviceIds.add(parts.id)
        }
        // Handled by the store read (latest) / componentDataApi (timeseries
        // via community widgets) — never pushed to tel/poll/ext.
        continue
      }
      if (ds.source === 'extension') { ext.push(ds); continue }
      if (ds.mode === 'timeseries' && ds.source !== 'system') { tel.push(ds); continue }
      if (ds.source === 'system' || ds.mode === 'list') { poll.push(ds) }
    }
    return { telemetrySources: tel, pollingSources: poll, extensionSources: ext, needsWebSocket: needsWs, relevantDeviceIds: deviceIds, deviceInfoIds: infoIds }
  }, [dataSources])

  const hasTelemetrySource = telemetrySources.length > 0
  const hasExtensionSource = extensionSources.length > 0

  const telemetryKey = useMemo(() => {
    return telemetrySources
      .map((ds) => {
        const isImg = isImageDataSource(ds)
        const actualTimeRange = ds.timeRange ?? (isImg ? 48 : 1)
        const actualLimit = ds.limit ?? (isImg ? 200 : 50)
        const actualAggregate = ds.aggregateExt ?? 'raw'
        const tw = ds.timeWindow
          ? `${ds.timeWindow.type}:${ds.timeWindow.startTime ?? ''}:${ds.timeWindow.endTime ?? ''}`
          : ''
        return `${getUnifiedId(ds)}|${getUnifiedField(ds)}|${actualTimeRange}|${actualLimit}|${actualAggregate}|${tw}`
      })
      .join('|')
  }, [telemetrySources])

  const pollingKey = useMemo(() => {
    return pollingSources
      .map((ds) => `${ds.source}|${getUnifiedId(ds) ?? ''}|${getUnifiedField(ds) ?? ''}|${ds.refresh ?? ''}`)
      .sort()
      .join('|')
  }, [pollingSources])

  const extensionKey = useMemo(() => {
    return extensionSources
      .map((ds) => {
        const tw = ds.timeWindow ? `${ds.timeWindow.type}:${ds.timeWindow.startTime ?? ''}:${ds.timeWindow.endTime ?? ''}` : ''
        return `${getUnifiedId(ds) ?? ''}:${getUnifiedField(ds) ?? ''}:${ds.timeRange ?? ''}:${ds.limit ?? ''}:${tw}`
      })
      .join('|')
  }, [extensionSources])

  const relevantExtensionIds = useMemo(() => {
    return new Set(extensionSources.map((ds) => getUnifiedId(ds)).filter(Boolean) as string[])
  }, [extensionSources])

  // ============================================================================
  // C. sendCommand
  // ============================================================================

  const sendCommand = useCallback(async (value?: unknown): Promise<boolean> => {
    if (!commandSource || !enabled) return false
    setSending(true)

    try {
      const deviceId = getUnifiedId(commandSource)
      const command = commandSource.field ?? (commandSource.command || 'setValue')
      if (!deviceId) {
        setErrorInternal('Command source has no device ID')
        return false
      }
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
      if (commandSource.commandParams) params = { ...params, ...commandSource.commandParams }

      const { api } = await import('@/lib/api')
      await api.sendCommand(deviceId, command, params)
      return true
    } catch (err) {
      setErrorInternal(err instanceof Error ? err.message : 'Command failed')
      return false
    } finally {
      setSending(false)
    }
  }, [commandSource, enabled])

  // ============================================================================
  // D-J. Sub-hook adapters (bridge to legacy sub-hook interface)
  // ============================================================================

  // Sub-hooks expect { startLoading, finishLoading, retryLoading, failLoading }
  const sourceAdapters = useMemo(() => ({
    startLoading,
    finishLoading,
    retryLoading: startLoading,
    failLoading: setErrorLoading,
    forceFinishLoading,
  }), [startLoading, finishLoading, setErrorLoading, forceFinishLoading])

  // Legacy setLoading adapter for sub-hooks that use it
  const legacySetLoading = useCallback((_l: boolean) => {
    // No-op: loading is managed via ref counter
  }, [])

  // Stable adapter — replaces 3 identical per-render closures (one per sub-hook)
  const setDataAdapter = useCallback((updater: unknown | ((prev: unknown) => unknown)) => {
    setData((prev) => typeof updater === 'function' ? (updater as (p: unknown) => unknown)(prev) as T : updater as T)
  }, [setData])

  // ============================================================================
  // D. Store source
  // ============================================================================

  const { readDataFromStore, wsConnected } = useStoreSource<T>(
    dataSources, currentKey, enabled,
    relevantDeviceIds, deviceInfoIds, hasTelemetrySource, needsWebSocket,
    {
      data, setData, setDataRaw, setLoading: legacySetLoading, setError, setLastUpdate,
      dataSourcesRef, optionsRef,
      sourceAdapters,
    },
    hasExtensionSource
  )

  // ============================================================================
  // F+G. Telemetry source
  // ============================================================================

  useTelemetrySource(telemetrySources, telemetryKey, enabled, hasTelemetrySource, relevantDeviceIds, wsConnected, {
    setData: setDataAdapter,
    setDataRaw, setLoading: legacySetLoading, setError, setLastUpdate, optionsRef,
    readDataFromStore,
    sourceAdapters,
  })

  // ============================================================================
  // H. Polling source (system, rule, message, http, etc.)
  // ============================================================================

  usePollingSource(pollingSources, pollingKey, enabled, {
    setData: setDataAdapter,
    setDataRaw, setLoading: legacySetLoading, setError, setLastUpdate, optionsRef,
    sourceAdapters,
  })

  // ============================================================================
  // I+K. Extension source
  // ============================================================================

  useExtensionSource(extensionSources, extensionKey, enabled, currentKey, relevantExtensionIds, {
    setData: setDataAdapter,
    setDataRaw, setLoading: legacySetLoading, setError, setLastUpdate, dataSourcesRef, optionsRef,
    sourceAdapters,
  })

  return {
    data,
    loading,
    error,
    lastUpdate,
    sources: dataSources,
    ...(hasCommandSource && { sendCommand, sending }),
  }
}

// ============================================================================
// Specialized hooks
// ============================================================================

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

  return { data: data ?? [], loading, error, lastUpdate }
}
