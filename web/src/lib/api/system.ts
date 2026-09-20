/** system api — split from api.ts 2026-09. */
import type { SearchResult, SearchSuggestion } from '@/types'
import { fetchAPI, getApiBase, getApiKey, tokenManager } from "./client"
import type { ServerUpgradeCheck, ServerUpgradeStatus } from "./types"

export const systemApi = {
  // ========== Stats API ==========
  getSystemStats: () => fetchAPI<{ version: string; uptime: number; platform: string; arch: string; cpu_count: number; total_memory: number; used_memory: number; free_memory: number; available_memory: number; cpu_usage: number; gpus: Array<{ name: string; vendor: string; total_memory_mb: number | null; driver_version: string | null }>; disks: Array<{ name: string; mount: string; total: number; used: number; available: number }>; networks: Array<{ name: string; ip: string; mac: string; rx_bytes: number; tx_bytes: number }> }>('/stats/system'),
  getRuleStats: () => fetchAPI<{ stats: { total_rules: number; enabled_rules: number; disabled_rules: number; by_type: Record<string, number> } }>('/stats/rules'),

  // ========== Server self-upgrade API (admin, browser/server deployments) ==========
  /**
   * Release check for the web-triggered server upgrade (About page).
   * `supported=false` with a `notes` hint for Docker/unsupported installs.
   * GET /api/system/upgrade/check
   */
  checkServerUpgrade: (force = false) =>
    fetchAPI<ServerUpgradeCheck>(`/system/upgrade/check${force ? '?force=true' : ''}`),
  /**
   * Kick off the staged server upgrade (single-flight on the server).
   * Progress flows via `SystemUpgradeProgress` WS events + status polling.
   * POST /api/system/upgrade
   */
  startServerUpgrade: (version?: string) =>
    fetchAPI<{ started: boolean; already_running: boolean }>('/system/upgrade', {
      method: 'POST',
      body: version ? JSON.stringify({ version }) : undefined,
    }),
  /** GET /api/system/upgrade/status — snapshot of the in-flight upgrade. */
  getServerUpgradeStatus: () => fetchAPI<ServerUpgradeStatus>('/system/upgrade/status'),

  /**
   * Download a ZIP archive of `neomind.log.*` files for diagnostic / support
   * flows. Triggers a browser download via blob URL.
   *
   * @param days Optional time-window filter. `1` = today only, `7` = last 7
   *   days, `30` = last 30 days. `undefined`/`0` = all time (default).
   * @returns the filename suggested by the server, or a fallback default.
   */
  downloadLogs: async (days?: number): Promise<{ filename: string; bytes: number }> => {
    const headers: Record<string, string> = {}
    const token = tokenManager.getToken()
    if (token) headers['Authorization'] = `Bearer ${token}`
    const apiKey = getApiKey()
    if (apiKey) headers['X-API-Key'] = apiKey

    const query = days && days > 0 ? `?days=${encodeURIComponent(days)}` : ''
    const response = await fetch(`${getApiBase()}/logs/download${query}`, { headers })
    if (!response.ok) {
      // Parse structured error JSON instead of dumping raw body to the toast —
      // matches `downloadMqttCaCert` pattern and avoids leaking backend
      // internals (file paths, stack traces) into user-visible notifications.
      try {
        const body = await response.json()
        const message =
          body?.error?.message || body?.message || `Failed to download logs (${response.status})`
        throw new Error(message)
      } catch (e) {
        if (e instanceof Error) throw e
        throw new Error(`Failed to download logs (${response.status})`)
      }
    }

    // Parse filename from Content-Disposition; fall back to timestamped default.
    const cd = response.headers.get('content-disposition') || ''
    const match = cd.match(/filename="?([^";]+)"?/i)
    const filename = match?.[1] || `neomind-logs-${new Date().toISOString().slice(0, 10)}.zip`

    const blob = await response.blob()
    // Trigger browser download via object URL.
    const url = URL.createObjectURL(blob)
    const a = document.createElement('a')
    a.href = url
    a.download = filename
    document.body.appendChild(a)
    a.click()
    a.remove()
    // Defer revoke so Safari has time to start the download.
    setTimeout(() => URL.revokeObjectURL(url), 1000)

    return { filename, bytes: blob.size }
  },


  // ========== Bulk Operations API ==========
  bulkCreateMessages: (messages: Array<{ title: string; message: string; severity?: string; category?: string }>) =>
    fetchAPI<{ created: number; ids: string[] }>('/bulk/messages', {
      method: 'POST',
      body: JSON.stringify({ messages }),
    }),
  bulkResolveMessages: (ids: string[]) =>
    fetchAPI<{ resolved: number }>('/messages/resolve', {
      method: 'POST',
      body: JSON.stringify({ message_ids: ids }),
    }),
  bulkAcknowledgeMessages: (ids: string[]) =>
    fetchAPI<{ acknowledged: number }>('/messages/acknowledge', {
      method: 'POST',
      body: JSON.stringify({ message_ids: ids }),
    }),
  bulkDeleteMessages: (ids: string[]) =>
    fetchAPI<{ deleted: number }>('/messages/delete', {
      method: 'POST',
      body: JSON.stringify({ message_ids: ids }),
    }),
  bulkDeleteSessions: (ids: string[]) =>
    fetchAPI<{ total: number; succeeded: number; failed: number }>('/bulk/sessions/delete', {
      method: 'POST',
      body: JSON.stringify({ session_ids: ids }),
    }),
  bulkDeleteDevices: (ids: string[]) =>
    fetchAPI<{ deleted?: number; succeeded?: number }>('/bulk/devices/delete', {
      method: 'POST',
      body: JSON.stringify({ device_ids: ids }),
    }),
  bulkDeleteDeviceTypes: (ids: string[]) =>
    fetchAPI<{
      deleted?: number
      succeeded?: number
      failed?: number
      results?: Array<{ success: boolean; id?: string; error?: string }>
    }>('/bulk/device-types/delete', {
      method: 'POST',
      body: JSON.stringify({ type_ids: ids }),
    }),
  bulkDeviceCommand: (deviceIds: string[], command: string, params: Record<string, unknown>) =>
    fetchAPI<{ results: Array<{ device_id: string; success: boolean }> }>('/bulk/devices/command', {
      method: 'POST',
      body: JSON.stringify({ device_ids: deviceIds, command, params }),
    }),

  // ========== Config Import/Export API ==========
  exportConfig: () =>
    fetchAPI<{ config: Record<string, unknown>; exported_at: number }>('/config/export'),
  importConfig: (config: Record<string, unknown>, merge?: boolean) =>
    fetchAPI<{ message: string; imported: number }>('/config/import', {
      method: 'POST',
      body: JSON.stringify({ config, merge }),
    }),
  validateConfig: (config: Record<string, unknown>) =>
    fetchAPI<{ valid: boolean; errors?: string[] }>('/config/validate', {
      method: 'POST',
      body: JSON.stringify(config),
    }),

  // ========== Search API ==========
  globalSearch: (q: string, types?: string[], limit?: number) =>
    fetchAPI<{ results: SearchResult[]; count: number }>(
      `/search?q=${encodeURIComponent(q)}${types ? `&types=${types.join(',')}` : ''}${limit ? `&limit=${limit}` : ''}`
    ),
  getSearchSuggestions: (q: string) =>
    fetchAPI<{ suggestions: SearchSuggestion[] }>(`/search/suggestions?q=${encodeURIComponent(q)}`),


}