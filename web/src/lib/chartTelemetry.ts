/**
 * Shared telemetry source conversion for chart components.
 *
 * Converts device/metric data sources into telemetry sources suitable
 * for fetching historical time-series data.
 *
 * Always fetches raw data points — aggregation is handled by the frontend
 * (chart components or telemetryTransform) so timestamps are preserved.
 */

import type { DataSource } from '@/types/dashboard'
import { getUnifiedId, getUnifiedField } from '@/types/dashboard'
import {
  getEffectiveTimeWindow,
  timeWindowToHours,
} from '@/lib/telemetryTransform'

/**
 * Convert a data source to a telemetry source for chart data fetching.
 * Handles telemetry, device, and metric source types.
 *
 * Always uses aggregate='raw' so the API returns data points with timestamps.
 * Frontend applies the user's chosen aggregation (via aggregateExt) locally.
 */
export function toTelemetrySource(
  dataSource?: DataSource,
  limit: number = 50,
  _timeRange: number = 1
): DataSource | undefined {
  if (!dataSource) return undefined

  const effectiveTimeWindow = dataSource.timeWindow ?? getEffectiveTimeWindow(dataSource)

  if (dataSource.type === 'telemetry') {
    return {
      ...dataSource,
      limit: dataSource.limit ?? limit,
      timeRange: dataSource.timeRange ?? timeWindowToHours(effectiveTimeWindow.type),
      timeWindow: effectiveTimeWindow,
      aggregate: 'raw',
      params: {
        ...dataSource.params,
        includeRawPoints: true,
      },
      transform: dataSource.transform ?? 'raw',
    }
  }

  const sourceId = getUnifiedId(dataSource)
  if ((dataSource.type === 'device' || dataSource.type === 'metric' || dataSource.mode === 'latest' || dataSource.mode === 'timeseries') && sourceId) {
    const field = getUnifiedField(dataSource) ?? 'value'
    return {
      type: 'telemetry' as const,
      sourceId,
      metricId: field,
      timeRange: dataSource.timeRange ?? timeWindowToHours(effectiveTimeWindow.type),
      limit: dataSource.limit ?? limit,
      aggregate: 'raw' as const,
      aggregateExt: dataSource.aggregateExt,
      timeWindow: effectiveTimeWindow,
      params: { includeRawPoints: true },
      transform: 'raw' as const,
      // Preserve unified fields for consistent routing
      source: 'device' as const,
      mode: 'timeseries' as const,
      id: sourceId,
      field,
    }
  }

  return dataSource
}
