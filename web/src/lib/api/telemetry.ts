/** telemetry api — split from api.ts 2026-09. */
import type { TelemetryDataResponse, TelemetrySummaryResponse, CommandHistoryResponse } from '@/types'
import { fetchAPI } from "./client"

/** Build query for GET /devices/:id/telemetry. Cursor wins over offset. */
export function buildDeviceTelemetryQuery(opts: {
  metric?: string
  start?: number
  end?: number
  limit?: number
  offset?: number
  bucketed?: boolean
  cursor?: number | null
}): URLSearchParams {
  const { metric, start, end, limit, offset, bucketed, cursor } = opts
  return new URLSearchParams({
    ...(metric && { metric }),
    ...(start && { start: start.toString() }),
    ...(end && { end: end.toString() }),
    ...(limit && { limit: limit.toString() }),
    ...(offset !== undefined && offset > 0 && cursor == null && { offset: offset.toString() }),
    ...(cursor != null && { cursor: cursor.toString() }),
    ...(bucketed && { bucketed: 'true' }),
  })
}

/**
 * Whole-window point count for a paged device-telemetry table.
 *
 * Offset pages report `pagination.total` for the whole window. A cursor page
 * scans only `[start, cursor]`, so its `total` is the points STRICTLY OLDER
 * than the cursor — this page plus the ones after it. A pager that requests
 * page N with `(N - 1) * pageSize` sends no offset alongside a cursor (the
 * backend echoes `offset: 0`), so a page past the first with a zero echo is a
 * cursor page: add back what was already paged.
 */
export function resolveTelemetryPageTotal(
  pagination: TelemetryDataResponse['pagination'],
  currentPage: number,
  pageSize: number,
): number | undefined {
  if (!pagination) return undefined
  const isCursorPage = currentPage > 1 && pagination.offset === 0
  if (!isCursorPage) return pagination.total
  return (currentPage - 1) * pageSize + pagination.total
}

export const telemetryApi = {
  // Device Telemetry
  // Prefer `cursor` (previous page's next_cursor) for sequential next-page;
  // `offset` remains correct for arbitrary page jumps after the storage-offset fix.
  getDeviceTelemetry: (
    deviceId: string,
    metric?: string,
    start?: number,
    end?: number,
    limit?: number,
    offset?: number,
    bucketed?: boolean,
    cursor?: number | null,
  ) =>
    fetchAPI<TelemetryDataResponse>(
      `/devices/${deviceId}/telemetry?${buildDeviceTelemetryQuery({
        metric,
        start,
        end,
        limit,
        offset,
        bucketed,
        cursor,
      })}`,
      {
        // [deleted-device 404] A device removed by another client (CLI,
        // second session) previously answered 200+empty here; since the
        // backend's 404 contract change this fires on EVERY poll cycle and
        // toasts forever. Dashboard code treats errors as empty data already
        // (useDataSource/fetch.ts catch), so the chart degrades gracefully —
        // only the global toast needs silencing.
        skipErrorToast: true,
      }
    ),
  getDeviceTelemetrySummary: (deviceId: string, hours?: number) =>
    fetchAPI<TelemetrySummaryResponse>(
      `/devices/${deviceId}/telemetry/summary${hours ? `?hours=${hours}` : ''}`
    ),
  getDeviceCommandHistory: (deviceId: string, limit?: number) =>
    fetchAPI<CommandHistoryResponse>(
      `/devices/${deviceId}/commands${limit ? `?limit=${limit}` : ''}`
    ),

}
