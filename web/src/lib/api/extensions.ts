/** extensions api — split from api.ts 2026-09. */
import type { Extension, ExtensionTypeDto, ExtensionLogEntry, ExtensionCapabilityDto, ExtensionHealthResponse, ExtensionCommandDescriptor, ExtensionExecuteRequest, ExtensionExecuteResponse, ExtensionDataSourceInfo, ExtensionQueryParams, ExtensionQueryResult, TransformDataSourceInfo, UnifiedDataSourceInfo } from '@/types'
import { fetchAPI, getApiBase, tokenManager } from "./client"

export const extensionsApi = {
  // ========== Extensions API ==========
  // Matches backend: crates/api/src/handlers/extensions.rs
  //
  // Extension system replaces the legacy Plugin system for dynamically loaded code.

  /**
   * List available extension types
   * GET /api/extensions/types
   */
  listExtensionTypes: () =>
    fetchAPI<ExtensionTypeDto[]>('/extensions/types'),

  /**
   * Get extension logs
   * GET /api/extensions/:id/logs
   */
  getExtensionLogs: (id: string) =>
    fetchAPI<ExtensionLogEntry[]>(`/extensions/${id}/logs`),

  /**
   * Clear extension logs
   * DELETE /api/extensions/:id/logs
   */
  clearExtensionLogs: (id: string) =>
    fetchAPI<{ message: string }>(`/extensions/${id}/logs`, { method: 'DELETE' }),

  /**
   * Unregister an extension
   * DELETE /api/extensions/:id
   */
  unregisterExtension: (id: string) =>
    fetchAPI<{ message: string; extension_id: string }>(`/extensions/${id}`, {
      method: 'DELETE',
    }),

  /**
   * Start an extension
   * POST /api/extensions/:id/start
   */
  startExtension: (id: string) =>
    fetchAPI<{ message: string; extension_id: string }>(`/extensions/${id}/start`, {
      method: 'POST',
    }),

  /**
   * Stop an extension
   * POST /api/extensions/:id/stop
   */
  stopExtension: (id: string) =>
    fetchAPI<{ message: string; extension_id: string }>(`/extensions/${id}/stop`, {
      method: 'POST',
    }),

  /**
   * Upload and install an extension package (.nep file)
   * POST /api/extensions/upload/file
   *
   * Uses JSON with base64-encoded file data (required by backend)
   * @param file - The .nep file to upload
   * @param signal - Optional AbortSignal for timeout/cancellation
   */
  uploadExtensionFile: async (file: File, signal?: AbortSignal): Promise<{
    extension_id: string
    name: string
    version: string
    message: string
  }> => {
    // Read file and convert to base64 using a reliable method
    const arrayBuffer = await file.arrayBuffer()
    const bytes = new Uint8Array(arrayBuffer)
    let binary = ''
    // Process in chunks to avoid call stack overflow for large files
    const chunkSize = 8192
    for (let i = 0; i < bytes.length; i += chunkSize) {
      const chunk = bytes.subarray(i, Math.min(i + chunkSize, bytes.length))
      binary += String.fromCharCode.apply(null, Array.from(chunk))
    }
    const base64Data = btoa(binary)

    // For large file uploads, use the dynamic API base (respects instance switching)
    const uploadApiBase = getApiBase()

    // Get auth token
    const token = tokenManager.getToken()
    const headers: Record<string, string> = {
      'Content-Type': 'application/json',
    }
    if (token) {
      headers['Authorization'] = `Bearer ${token}`
    }

    const response = await fetch(`${uploadApiBase}/extensions/upload/file`, {
      method: 'POST',
      headers,
      body: JSON.stringify({
        data: base64Data,
        filename: file.name,
      }),
      signal,
    })

    // Handle error responses
    if (!response.ok) {
      const text = await response.text()
      let errorMessage = `Upload failed: ${response.status}`
      try {
        const json = JSON.parse(text)
        if (json.message) {
          errorMessage = json.message
        }
      } catch {
        if (text) errorMessage = text
      }
      throw new Error(errorMessage)
    }

    return response.json()
  },

  /**
   * Reload an extension from file
   * POST /api/extensions/:id/reload
   */
  reloadExtension: (id: string) =>
    fetchAPI<{ message: string; extension_id: string; config_applied: boolean }>(`/extensions/${id}/reload`, {
      method: 'POST',
    }),

  /**
   * Toggle an extension's master tool-enable flag.
   * PATCH /api/extensions/:id/enabled
   * `enabled=false` hides ALL of this extension's tools from the LLM.
   */
  setExtensionEnabled: (id: string, enabled: boolean) =>
    fetchAPI<{ id: string; enabled: boolean }>(`/extensions/${id}/enabled`, {
      method: 'PATCH',
      body: JSON.stringify({ enabled }),
    }),

  /**
   * Toggle a single extension command's tool-enable flag.
   * PATCH /api/extensions/:id/commands/:cmd/enabled
   * Independent of the master `enabled` flag.
   */
  setExtensionCommandEnabled: (id: string, cmd: string, enabled: boolean) =>
    fetchAPI<{ id: string; command: string; enabled: boolean }>(
      `/extensions/${id}/commands/${encodeURIComponent(cmd)}/enabled`,
      {
        method: 'PATCH',
        body: JSON.stringify({ enabled }),
      },
    ),

  /**
   * Check extension health
   * GET /api/extensions/:id/health
   */
  getExtensionHealth: (id: string) =>
    fetchAPI<ExtensionHealthResponse>(`/extensions/${id}/health`),

  /**
   * Execute a command on an extension (legacy endpoint)
   * POST /api/extensions/:id/command
   */
  executeExtensionCommand: (id: string, command: string, args?: Record<string, unknown>, opts?: { skipErrorToast?: boolean }) =>
    fetchAPI<Record<string, unknown>>(`/extensions/${id}/command`, {
      method: 'POST',
      body: JSON.stringify({ command, args }),
      // Background fetches (e.g. dashboard auto-refresh) may invoke commands that
      // fail legitimately (required param not yet configured). Suppress the global
      // error toast there — callers already fall back to queryData/empty data.
      skipErrorToast: opts?.skipErrorToast,
    }),

  /**
   * Get all extension capabilities (for Agent tools, Transform operations, etc.)
   * GET /api/extensions/capabilities
   */
  getExtensionCapabilities: () =>
    fetchAPI<ExtensionCapabilityDto[]>('/extensions/capabilities'),

  /**
   * Get a specific extension's capabilities
   * GET /api/extensions/:id/capabilities
   */
  getExtensionCapabilitiesById: (id: string) =>
    fetchAPI<ExtensionCapabilityDto>(`/extensions/${id}/capabilities`),

  /**
   * Invoke an extension command (newer endpoint with better error handling)
   * POST /api/extensions/:id/invoke
   */
  invokeExtension: (id: string, command: string, args?: Record<string, unknown>) =>
    fetchAPI<Record<string, unknown>>(`/extensions/${id}/invoke`, {
      method: 'POST',
      body: JSON.stringify({ command, args }),
    }),

  // ========== Extension API (unified command-based) ==========

  /**
   * List all extensions with their commands
   * GET /api/extensions
   */
  listExtensions: (params?: {
    state?: string
  }) =>
    fetchAPI<Extension[]>(
      `/extensions${params ? `?${new URLSearchParams(
        Object.entries(params).reduce((acc, [key, value]) => {
          if (value !== undefined) acc[key] = String(value)
          return acc
        }, {} as Record<string, string>)
      )}` : ''}`
    ),

  /**
   * Get a specific extension with its commands
   * GET /api/extensions/:id
   */
  getExtension: (id: string) =>
    fetchAPI<Extension>(`/extensions/${id}`),

  /**
   * Get commands for an extension
   * GET /api/extensions/:id/commands
   */
  listCommands: (id: string) =>
    fetchAPI<ExtensionCommandDescriptor[]>(`/extensions/${id}/commands`),

  /**
   * Execute an extension command
   * POST /api/extensions/:id/command
   */
  executeCommand: (id: string, request: ExtensionExecuteRequest) =>
    fetchAPI<ExtensionExecuteResponse>(`/extensions/${id}/command`, {
      method: 'POST',
      body: JSON.stringify(request),
    }),

  /**
   * Get data sources for an extension
   * GET /api/extensions/:id/data-sources
   */
  listDataSources: (id: string) =>
    fetchAPI<ExtensionDataSourceInfo[]>(`/extensions/${id}/data-sources`),

  /**
   * Get historical data for an extension metric
   * GET /api/extensions/:id/metrics/:metric/data?start=&end=&limit=
   */
  getMetricData: (extensionId: string, metric: string, params?: { start?: number; end?: number; limit?: number }) =>
    fetchAPI<{
      source_id: string
      extension_id: string
      metric: string
      start: number
      end: number
      count: number
      data: Array<{ timestamp: number; value: unknown; quality: string }>
    }>(`/extensions/${extensionId}/metrics/${metric}/data${params ? `?${new URLSearchParams(params as any).toString()}` : ''}`),

  /**
   * Query data from an extension
   * Uses getMetricData to fetch historical data
   */
  queryData: async (params: ExtensionQueryParams): Promise<ExtensionQueryResult> => {
    const { extension_id, command: _command, field: metric, start_time, end_time, limit } = params

    try {
      // Convert milliseconds to seconds for backend (backend expects seconds)
      const start_sec = start_time !== undefined ? Math.floor(start_time / 1000) : undefined
      const end_sec = end_time !== undefined ? Math.floor(end_time / 1000) : undefined

      const result = await fetchAPI<{
        source_id: string
        extension_id: string
        metric: string
        start: number
        end: number
        count: number
        data: Array<{ timestamp: number; value: unknown; quality: string }>
      }>(`/extensions/${extension_id}/metrics/${metric}/data${start_sec !== undefined || end_sec !== undefined || limit !== undefined ? `?${new URLSearchParams({
          ...(start_sec !== undefined && { start: start_sec.toString() }),
          ...(end_sec !== undefined && { end: end_sec.toString() }),
          ...(limit !== undefined && { limit: limit.toString() }),
        } as any).toString()}` : ''}`)

      return {
        source_id: result.source_id,
        data_points: result.data.map(p => ({
          timestamp: p.timestamp,
          value: p.value as any,
        })),
      }
    } catch (err) {
      console.error('[API] Failed to query extension data:', err)
      return {
        source_id: `${params.extension_id}:${params.command}:${params.field}`,
        data_points: [],
      }
    }
  },

  /**
   * Get all extension + transform data sources (for dashboard selectors, rules, etc.)
   * Reuses listUnifiedDataSources internally to avoid duplicate API calls.
   */
  listAllDataSources: async () => {
    try {
      const result = await extensionsApi.listUnifiedDataSources({ limit: 500, skip_telemetry: 'true' })
      const mapped: Array<ExtensionDataSourceInfo | TransformDataSourceInfo> = []

      for (const ds of result.data) {
        if (ds.source_type === 'extension') {
          mapped.push({
            id: ds.id,
            extension_id: ds.source_name,
            command: '',
            field: ds.field,
            display_name: ds.source_display_name + ': ' + ds.field_display_name,
            data_type: (ds.data_type as any) || 'float',
            unit: ds.unit,
            description: ds.description || ds.field_display_name,
            aggregatable: true,
            default_agg_func: 'last' as const,
          })
        } else if (ds.source_type === 'transform') {
          mapped.push({
            id: ds.id,
            transform_id: ds.source_name,
            transform_name: ds.source_display_name,
            metric_name: ds.field,
            display_name: ds.field_display_name,
            data_type: ds.data_type,
            unit: ds.unit,
            description: ds.description || '',
            last_update: ds.last_update ?? undefined,
          })
        }
      }

      return mapped
    } catch {
      return [] as Array<ExtensionDataSourceInfo | TransformDataSourceInfo>
    }
  },

  /**
   * Get unified data sources from the /api/data/sources endpoint
   * Aggregates all data sources (devices, extensions, transforms, AI metrics) in a single call.
   * Supports server-side filtering and pagination.
   */
  listUnifiedDataSources: (params?: Record<string, string | number>, signal?: AbortSignal) => {
    const qs = params && Object.keys(params).length > 0
      ? new URLSearchParams(
          Object.entries(params).map(([k, v]) => [k, String(v)])
        ).toString()
      : ''
    return fetchAPI<{ data: UnifiedDataSourceInfo[]; total: number; source_options: [string, string][] }>(qs ? `/data/sources?${qs}` : '/data/sources', { signal })
  },

  /**
   * Query telemetry time-series data for any source type
   * GET /api/telemetry?source=...&metric=...&start=...&end=...&limit=...
   */
  /**
   * Aggregate a telemetry series over a time range.
   * GET /api/telemetry?source=...&metric=...&start=...&end=...&aggregate=avg
   */
  aggregateTelemetry: (source: string, metric: string, start: number, end: number, agg: 'avg' | 'min' | 'max' | 'sum' | 'count') => {
    const qs = new URLSearchParams({ source, metric, start: String(start), end: String(end), aggregate: agg }).toString()
    return fetchAPI<{ value: number | null; count: number }>(`/telemetry?${qs}`)
  },

  queryTelemetry: (source: string, metric: string, start: number, end: number, limit?: number, bucketed?: boolean, offset?: number) => {
    const qs = new URLSearchParams({
      source, metric,
      start: String(start),
      end: String(end),
      ...(limit ? { limit: String(limit) } : {}),
      ...(bucketed ? { bucketed: 'true' } : {}),
      ...(offset ? { offset: String(offset) } : {}),
    }).toString()
    return fetchAPI<{ source_id: string; data: Array<{ timestamp: number; value: unknown; quality: number | null }>; count: number; total_count?: number }>(`/telemetry?${qs}`)
  },

}
