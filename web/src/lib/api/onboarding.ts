/** onboarding api — split from api.ts 2026-09. */
import type { NotificationMessage } from '@/types'
import { fetchAPI } from "./client"

export const onboardingApi = {
  // ========== Auto-onboarding Configuration ==========
  // Get auto-onboarding configuration (simplified to 3 fields)
  getOnboardConfig: () =>
    fetchAPI<{
      enabled: boolean
      max_samples: number
      draft_retention_secs: number
    }>('/devices/drafts/config'),

  // Update auto-onboarding configuration
  updateOnboardConfig: (config: {
    enabled?: boolean
    max_samples?: number
    draft_retention_secs?: number
  }) =>
    fetchAPI<{ message: string }>('/devices/drafts/config', {
      method: 'PUT',
      body: JSON.stringify(config),
    }),

  // Upload device data for auto-onboarding analysis
  uploadDeviceData: (request: {
    device_id?: string
    source?: string
    data: unknown[]
  }) =>
    fetchAPI<{ message: string }>('/devices/drafts/upload', {
      method: 'POST',
      body: JSON.stringify(request),
    }),

  // Messages (replaces Alerts) - response format: { messages: NotificationMessage[], total: number }
  getMessages: (params?: Record<string, string>) =>
    fetchAPI<{ messages: NotificationMessage[]; total: number }>(
      `/messages${params ? `?${new URLSearchParams(params)}` : ''}`
    ),
  getMessage: (id: string) => fetchAPI<NotificationMessage>(`/messages/${id}`),
  createMessage: (req: {
    category?: string
    title: string
    message: string
    severity?: string
    source?: string
    source_type?: string
    source_id?: string
    tags?: string[]
    message_type?: string
    payload?: Record<string, unknown>
    metadata?: Record<string, unknown>
  }) =>
    fetchAPI<{ id: string; message: string; message_zh: string }>('/messages', {
      method: 'POST',
      body: JSON.stringify({
        category: req.category || 'alert',
        title: req.title,
        message: req.message,
        severity: req.severity || 'info',
        source: req.source || 'api',
        source_type: req.source_type,
        source_id: req.source_id,
        tags: req.tags,
        message_type: req.message_type,
        payload: req.payload,
        metadata: req.metadata,
      }),
    }),
  acknowledgeMessage: (id: string) =>
    fetchAPI<{ acknowledged: boolean; message_id: string }>(`/messages/${id}/acknowledge`, {
      method: 'POST',
    }),
  resolveMessage: (id: string) =>
    fetchAPI<{ resolved: boolean; message_id: string }>(`/messages/${id}/resolve`, {
      method: 'POST',
    }),
  archiveMessage: (id: string) =>
    fetchAPI<{ archived: boolean; message_id: string }>(`/messages/${id}/archive`, {
      method: 'POST',
    }),
  deleteMessage: (id: string) =>
    fetchAPI<{ message: string; message_zh: string }>(`/messages/${id}`, {
      method: 'DELETE',
    }),
  getMessageStats: () => fetchAPI<{ total: number; active: number; by_category: Record<string, number>; by_severity: Record<string, number>; by_status: Record<string, number> }>('/messages/stats'),

}
