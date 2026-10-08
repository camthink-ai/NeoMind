/** sessions api — split from api.ts 2026-09. */
import type { ChatSession, SessionHistoryResponse } from '@/types'
import { fetchAPI, type FetchOptions } from "./client"

export const sessionsApi = {
  // Sessions
  // Note: Backend returns paginated response with data as array (auto-unwrapped by fetchAPI)
  listSessions: (page = 1, pageSize = 10) =>
    fetchAPI<ChatSession[]>(`/sessions?page=${page}&page_size=${pageSize}`),
  createSession: (sessionConfig?: { systemPromptSuffix?: string; allowedTools?: string[] }) =>
    fetchAPI<{ sessionId: string }>('/sessions', {
      method: 'POST',
      body: sessionConfig ? JSON.stringify({ sessionConfig }) : undefined,
    }),
  getSession: (id: string) => fetchAPI<{ sessionId: string; state: { id: string; created_at: number; last_activity: number; message_count: number } }>(`/sessions/${id}`),
  updateSession: (id: string, title?: string) =>
    fetchAPI<{ sessionId: string; updated: boolean }>(`/sessions/${id}`, {
      method: 'PUT',
      body: JSON.stringify({ title }),
    }),
  getSessionHistory: (id: string, query?: { limit?: number; before?: number }, options?: FetchOptions) => {
    const qs = query
      ? new URLSearchParams(
          Object.entries(query)
            .filter(([, v]) => v != null)
            .map(([k, v]) => [k, String(v)])
        ).toString()
      : ''
    return fetchAPI<SessionHistoryResponse>(`/sessions/${id}/history${qs ? `?${qs}` : ''}`, options)
  },
  deleteSession: (id: string) =>
    fetchAPI<{ deleted: boolean; sessionId: string }>(`/sessions/${id}`, {
      method: 'DELETE',
    }),

  // Manual context compaction — force a conversation summary now, bypassing
  // the auto 60% threshold (the "/compact" analog for when the model starts
  // forgetting).
  compactSession: (id: string) =>
    fetchAPI<{
      sessionId: string
      summarizedMessages: number
      newUpToIndex: number
      fallbackUsed: boolean
    }>(`/sessions/${id}/compact`, {
      method: 'POST',
    }),

  // Clear conversation history but keep the session (the "/clear" analog).
  // Also resets any stored summary server-side.
  clearSessionHistory: (id: string) =>
    fetchAPI<{ sessionId: string; cleared: boolean }>(`/sessions/${id}/clear`, {
      method: 'POST',
    }),

  // Pending stream recovery (for WebSocket reconnection)
  getPendingStream: (id: string) =>
    fetchAPI<{ hasPending: boolean; sessionId: string; userMessage?: string; content?: string; thinking?: string; stage?: string; elapsed?: number; startedAt?: number }>(`/sessions/${id}/pending`),
  clearPendingStream: (id: string) =>
    fetchAPI<{ cleared: boolean; sessionId: string }>(`/sessions/${id}/pending`, {
      method: 'DELETE',
    }),

}
