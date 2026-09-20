/** telemetry api — split from api.ts 2026-09. */
import type { TelemetryDataResponse, TelemetrySummaryResponse, CommandHistoryResponse } from '@/types'
import { fetchAPI } from "./client"

export const telemetryApi = {
  // Device Telemetry
  getDeviceTelemetry: (deviceId: string, metric?: string, start?: number, end?: number, limit?: number, offset?: number, bucketed?: boolean) =>
    fetchAPI<TelemetryDataResponse>(
      `/devices/${deviceId}/telemetry?${new URLSearchParams({
        ...(metric && { metric }),
        ...(start && { start: start.toString() }),
        ...(end && { end: end.toString() }),
        ...(limit && { limit: limit.toString() }),
        ...(offset !== undefined && offset > 0 && { offset: offset.toString() }),
        ...(bucketed && { bucketed: 'true' }),
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
