/**
 * Component Data API — pure async functions for fetching data outside React hooks.
 *
 * Used by ComponentRenderer to inject a `fetchData` prop into community/extension components,
 * giving them a unified data access API without needing useDataSource.
 */

import type { DataSource, DataSourceMode, DataSourceSource } from '@/types/dashboard'
import { normalizeDataSource, getUnifiedSource, getUnifiedMode, getUnifiedId, getUnifiedField } from '@/types/dashboard'
import { useStore } from '@/store'
import { findDevice } from '@/lib/deviceUtils'
import { extractValueFromData, resolveDeviceInfoValue } from '@/hooks/useDataSource/helpers'
import { fetchHistoricalTelemetry, fetchSystemStats } from '@/hooks/useDataSource/fetch'

export interface FetchDataResult {
  value?: unknown       // mode=latest: single value
  series?: unknown[]    // mode=timeseries: time-series array
}

/**
 * Resolve data for a single DataSource (pure async, no React hook).
 */
export async function resolveDataSourceData(
  ds: DataSource,
  options?: { timeRange?: number; limit?: number }
): Promise<FetchDataResult> {
  const source = getUnifiedSource(ds) ?? inferSourceFromType(ds.type)
  const mode = getUnifiedMode(ds) ?? inferModeFromType(ds.type, ds)
  const id = getUnifiedId(ds) ?? ds.sourceId ?? ''
  const field = getUnifiedField(ds) ?? ''

  switch (mode) {
    case 'latest': {
      if (source === 'expression') {
        // Inline computed value — resolve each ref from live store telemetry
        // (same store the device branch reads), then evaluate.
        const expr = ds.expr ?? ''
        if (!expr) return { value: undefined }
        const { parseExprRefs, refParts, evalExpression } = await import('@/lib/expressionSource')
        const storeState = useStore.getState()
        const values: Record<string, number> = {}
        for (const ref of parseExprRefs(expr)) {
          const parts = refParts(ref)
          if (!parts) continue
          const cv = storeState.deviceTelemetry[parts.id]
          const raw = cv !== undefined ? extractValueFromData(cv, parts.field) : undefined
          const num = typeof raw === 'number' ? raw : Number(raw)
          if (Number.isFinite(num)) values[ref] = num
        }
        const result = evalExpression(expr, values)
        return result.value === null ? { value: undefined } : { value: result.value }
      }
      if (source === 'device') {
        const storeState = useStore.getState()
        const device = findDevice(storeState.devices, id)
        // Read telemetry from split map, fallback to device.current_values
        const cv = storeState.deviceTelemetry[id] || device?.current_values
        if (cv && typeof cv === 'object') {
          const value = extractValueFromData(cv, field)
          if (value !== undefined) return { value }
        }
        return { value: undefined }
      }
      if (source === 'extension') {
        try {
          const { api } = await import('@/lib/api')
          const result = await api.executeExtensionCommand(id, field, {})
          const resultData = (result as Record<string, unknown>).result ?? result
          return { value: resultData }
        } catch {
          return { value: undefined }
        }
      }
      if (source === 'system') {
        try {
          const response = await fetchSystemStats(field)
          return { value: response.data }
        } catch {
          return { value: undefined }
        }
      }
      break
    }
    case 'timeseries': {
      if (source === 'expression') {
        // Per-timestamp evaluation over forward-fill aligned history of
        // every referenced metric.
        const expr = ds.expr ?? ''
        if (!expr) return { series: [] }
        const timeRange = options?.timeRange ?? (ds.timeRange && ds.timeRange > 0 ? ds.timeRange : 24)
        const limit = options?.limit ?? ds.limit ?? 50
        const { parseExprRefs, refParts, evalExpressionSeries } = await import('@/lib/expressionSource')
        const seriesByRef: Record<string, { t: number; v: number }[]> = {}
        for (const ref of parseExprRefs(expr)) {
          const parts = refParts(ref)
          if (!parts) continue
          try {
            const response = await fetchHistoricalTelemetry(parts.id, parts.field, timeRange, limit, 'raw', true, true)
            const raw = (response.raw ?? response.data) as Array<{ timestamp: number; value: number } | number>
            const pts = (Array.isArray(raw) ? raw : [])
              .map((p) =>
                typeof p === 'number'
                  ? { t: NaN, v: p }
                  : { t: (p.timestamp as number) * 1000, v: Number(p.value) }
              )
              .filter((p) => Number.isFinite(p.t) && Number.isFinite(p.v))
              .sort((a, b) => a.t - b.t)
            if (pts.length > 0) seriesByRef[ref] = pts
          } catch { /* ref without history — evalExpressionSeries reports */ }
        }
        const { series } = evalExpressionSeries(expr, seriesByRef)
        return { series: series.map((p) => ({ timestamp: p.t / 1000, value: p.v })) }
      }
      const timeRange = options?.timeRange ?? (ds.timeRange && ds.timeRange > 0 ? ds.timeRange : 24)
      const limit = options?.limit ?? ds.limit ?? 50
      try {
        const response = await fetchHistoricalTelemetry(id, field, timeRange, limit, 'raw', true, true)
        // Prefer raw points (with timestamps) over flat number array
        return { series: response.raw && response.raw.length ? response.raw : response.data }
      } catch {
        return { series: [] }
      }
    }
    case 'command':
      return { value: undefined }
    case 'info': {
      if (source === 'device') {
        const device = findDevice(useStore.getState().devices, id)
        return { value: resolveDeviceInfoValue(device, field, '-') }
      }
      break
    }
  }
  return {}
}

/**
 * Fetch data for a DataSource (single or array).
 * Returns a single result for single sources, or an array for multiple.
 */
export async function resolveComponentData(
  dataSource: DataSource | DataSource[],
  options?: { timeRange?: number; limit?: number }
): Promise<FetchDataResult | FetchDataResult[]> {
  const sources = normalizeDataSource(dataSource)
  if (sources.length === 1) {
    return resolveDataSourceData(sources[0], options)
  }
  return Promise.all(sources.map(ds => resolveDataSourceData(ds, options)))
}

// ============================================================================
// Inference helpers (fallback when unified fields are absent)
// ============================================================================

function inferSourceFromType(type: string): DataSourceSource {
  switch (type) {
    case 'device': case 'metric': case 'command': case 'telemetry': case 'device-info':
      return 'device'
    case 'extension': case 'extension-metric': case 'extension-command':
      return 'extension'
    case 'system':
      return 'system'
    case 'transform':
      return 'transform'
    case 'agent':
      return 'ai'
    default:
      return 'device'
  }
}

function inferModeFromType(type: string, ds: DataSource): DataSourceMode {
  switch (type) {
    case 'device': case 'metric': return 'latest'
    case 'command': case 'extension-command': return 'command'
    case 'device-info': return 'info'
    case 'telemetry': case 'transform': case 'extension-metric': return 'timeseries'
    case 'extension':
      return (ds.timeRange || ds.timeWindow) ? 'timeseries' : 'latest'
    case 'system': case 'agent': return 'latest'
    default: return 'latest'
  }
}
