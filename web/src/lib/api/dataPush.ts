/** dataPush api — split from api.ts 2026-09. */
import { fetchAPI } from "./client"

export const dataPushApi = {
  // ========== Data Push API ==========

  /** List all push targets */
  listPushTargets: () =>
    fetchAPI<{ targets: import('@/types').PushTarget[]; total: number }>('/data-push'),

  /** Get a push target by ID */
  getPushTarget: (id: string) =>
    fetchAPI<import('@/types').PushTarget>(`/data-push/${id}`),

  /** Create a new push target */
  createPushTarget: (data: import('@/types').CreatePushTargetRequest) =>
    fetchAPI<{ id: string; name: string; target_type: string; enabled: boolean }>('/data-push', {
      method: 'POST',
      body: JSON.stringify(data),
    }),

  /** Update a push target */
  updatePushTarget: (id: string, data: import('@/types').UpdatePushTargetRequest) =>
    fetchAPI<{ id: string; name: string; enabled: boolean }>(`/data-push/${id}`, {
      method: 'PUT',
      body: JSON.stringify(data),
    }),

  /** Delete a push target */
  deletePushTarget: (id: string) =>
    fetchAPI<{ message: string }>(`/data-push/${id}`, {
      method: 'DELETE',
    }),

  /** Test a push target */
  testPushTarget: (id: string) =>
    fetchAPI<import('@/types').DeliveryLog>(`/data-push/${id}/test`, {
      method: 'POST',
      body: JSON.stringify({}),
    }),

  /** Start a push target */
  startPushTarget: (id: string) =>
    fetchAPI<{ message: string }>(`/data-push/${id}/start`, {
      method: 'POST',
      body: JSON.stringify({}),
    }),

  /** Stop a push target */
  stopPushTarget: (id: string) =>
    fetchAPI<{ message: string }>(`/data-push/${id}/stop`, {
      method: 'POST',
      body: JSON.stringify({}),
    }),

  /** List delivery logs for a push target */
  listPushDeliveryLogs: (id: string, limit?: number, offset?: number) => {
    const params = new URLSearchParams()
    if (limit) params.set('limit', String(limit))
    if (offset) params.set('offset', String(offset))
    const qs = params.toString() ? `?${params.toString()}` : ''
    return fetchAPI<{ logs: import('@/types').DeliveryLog[]; total: number }>(`/data-push/${id}/logs${qs}`)
  },

  /** Get push statistics */
  getPushStats: () =>
    fetchAPI<import('@/types').PushStats>('/data-push/stats'),
}
