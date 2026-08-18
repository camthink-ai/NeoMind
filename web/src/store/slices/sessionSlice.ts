/**
 * Session Slice
 *
 * Handles chat session management and message history.
 * Supports multiple sessions with switching capability.
 */

import type { StateCreator } from 'zustand'
import type { Message } from '@/types'
import { logError } from '@/lib/errors'
import { mergeMessagesForDisplay as mergeAssistantMessages } from '@/lib/messageUtils'

import type { SessionState } from '../types'
import type { ChatSession } from '@/types'
import { api } from '@/lib/api'
// Must match PANEL_SESSION_PREFIX in pageAssistant.ts — panel sessions are
// stored per page (each carries its own system-prompt suffix / tool profile)
import { PANEL_SESSION_PREFIX } from '@/components/chat/pageAssistant'
import { normalizeSessions, normalizeSessionsResponse } from '@/lib/api/transforms'
import { fetchCache } from '@/lib/utils/async'

export interface SessionSlice extends SessionState {
  // Actions
  setSessionId: (id: string) => void
  addMessage: (message: Message) => void
  clearMessages: () => void
  createSession: () => Promise<string | null>
  switchSession: (sessionId: string) => Promise<void>
  deleteSession: (sessionId: string) => Promise<void>
  clearAllSessions: () => Promise<void>
  updateSessionTitle: (sessionId: string, title: string) => Promise<void>
  loadSessions: () => Promise<void>
  loadMoreSessions: () => Promise<void>
  fetchSessionHistory: (sessionId: string) => Promise<void>
}

export const createSessionSlice: StateCreator<
  SessionSlice,
  [],
  [],
  SessionSlice
> = (set, get) => ({
  // Initial state
  sessionId: null,
  messages: [],
  sessions: [],
  sessionsPage: 1,
  sessionsHasMore: true,
  sessionsLoading: false,
  isLoadingSession: false,

  // Actions
  setSessionId: (id: string) => {
    set({ sessionId: id })
  },

  addMessage: (message: Message) => {
    // Invalidate session history cache so re-fetching shows new messages
    const currentSessionId = get().sessionId
    if (currentSessionId) {
      fetchCache.invalidate(`sessionHistory:${currentSessionId}`)
    }

    set((state) => {
      // If message with same ID exists, update it (especially important for partial->final transition)
      const existingIndex = state.messages.findIndex(m => m.id === message.id)

      if (existingIndex !== -1) {
        // Update existing message in place
        // This handles both: partial->partial updates AND partial->final transition
        const updatedMessages = [...state.messages]
        updatedMessages[existingIndex] = message
        return { messages: updatedMessages }
      }

      // Otherwise append new message
      const newMessages = [...state.messages, message]

      // Update current session in sessions list using splice to avoid O(n) map
      let updatedSessions = state.sessions
      const sessionIdx = state.sessions.findIndex(s => s.sessionId === state.sessionId)
      if (sessionIdx !== -1) {
        const s = state.sessions[sessionIdx]
        const preview = message.role === 'user'
          ? message.content
          : (s.preview || (message.content ? message.content.slice(0, 50) + '...' : ''))

        updatedSessions = [...state.sessions]
        updatedSessions[sessionIdx] = {
          ...s,
          preview,
          messageCount: (s.messageCount || 0) + 1,
          updatedAt: Date.now(),
        }
      }

      return {
        messages: newMessages,
        sessions: updatedSessions,
      }
    })
  },

  clearMessages: () => {
    set({ messages: [] })
  },

  createSession: async () => {
    fetchCache.invalidate('sessions')
    try {
      const result = await api.createSession()

      // Reload sessions from server to ensure consistency
      // This avoids issues where local state diverges from server state
      const listResult = await api.listSessions(1, 100)
      const sessions = normalizeSessionsResponse(listResult)

      set({
        sessionId: result.sessionId,
        messages: [],
        sessions,
      })

      // Update WebSocket to use the new session
      const { ws } = await import('@/lib/websocket')
      ws.setSessionId(result.sessionId)

      return result.sessionId
    } catch (error) {
      logError(error, { operation: 'Create session' })
      return null
    }
  },

  switchSession: async (sessionId: string) => {
    // Check if we're already on this session WITH loaded messages
    // If sessionId matches but messages are empty, we still need to load (e.g. sidebar click)
    const state = get()
    if (sessionId === state.sessionId && state.messages.length > 0 && !state.isLoadingSession) {
      return
    }

    // Set loading state immediately for responsive UI
    set({ isLoadingSession: true })

    // IMPORTANT: Update WebSocket FIRST before any API calls
    // This ensures any subsequent messages go to the correct session
    const { ws } = await import('@/lib/websocket')
    ws.setSessionId(sessionId)

    try {
      // Fetch the session history — skip global error toast since we handle recovery below
      const historyResult = await api.getSessionHistory(sessionId, { skipErrorToast: true })

      // Validate the response before processing
      if (!historyResult) {
        console.warn(`Session ${sessionId} returned empty result from API`)
        set({ sessionId, messages: [], isLoadingSession: false })
        return
      }

      // Merge fragmented assistant messages from backend
      // Backend stores: [msg1(thinking+tools)] + [tool results] + [msg2(content only)]
      // Frontend expects: [msg1(thinking+tools+content)]
      const messages = historyResult.messages || []
      const mergedMessages = mergeAssistantMessages(messages)

      set({
        sessionId,
        messages: mergedMessages,
        isLoadingSession: false,
      })
    } catch (error: any) {
      logError(error, { operation: 'Switch session' })

      // If session not found (404), show error but don't auto-delete
      // This prevents data loss when database has inconsistencies
      if (error?.error?.code === 'NOT_FOUND' || error?.status === 404) {
        console.warn(`Session ${sessionId} not found in database. This may indicate data corruption.`)

        // Try to reload sessions from server to get accurate list
        try {
          const result = await api.listSessions(1, 100)
          const sessions = normalizeSessionsResponse(result)

          // Only update sessions if server returned a valid list
          if (sessions.length > 0) {
            set((state) => ({
              sessions,
              // Keep current session if it still exists, otherwise use first session
              sessionId: sessions.some(s => s.sessionId === sessionId)
                ? state.sessionId
                : sessions[0].sessionId,
              messages: sessions.some(s => s.sessionId === sessionId)
                ? state.messages
                : [],
              isLoadingSession: false,
            }))
          } else {
            // No sessions on server - keep local state and notify user
            logError(new Error('No sessions found on server'), { operation: 'Switch session - database check' })
            set({ isLoadingSession: false })
          }
        } catch (loadError) {
          logError(loadError, { operation: 'Reload sessions after switch' })
          set({ isLoadingSession: false })
        }
        return
      }

      // For other errors, just keep current state
      logError(error, { operation: 'Switch session - general error' })
      set({ isLoadingSession: false })
    }
  },

  deleteSession: async (sessionIdToDelete: string) => {
    fetchCache.invalidate('sessions')

    // If the deleted session is any page's persisted panel session, clear
    // that bucket so the floating chat doesn't try to switch to a deleted
    // session. Panel sessions are keyed per page — scan the prefix.
    try {
      for (let i = localStorage.length - 1; i >= 0; i--) {
        const k = localStorage.key(i)
        if (k && k.startsWith(PANEL_SESSION_PREFIX) && localStorage.getItem(k) === sessionIdToDelete) {
          localStorage.removeItem(k)
        }
      }
    } catch { /* localStorage may be unavailable */ }

    try {
      await api.deleteSession(sessionIdToDelete)

      // After successful deletion, reload sessions from server
      // This ensures consistency between server and client state
      const result = await api.listSessions(1, 100)
      const sessions = normalizeSessionsResponse(result)

      set((state) => {
        const wasCurrentSession = state.sessionId === sessionIdToDelete

        // If no sessions left, return empty list
        if (sessions.length === 0) {
          return {
            sessions: [],
            sessionId: null,
            messages: [],
          }
        }

        // If we deleted the current session, switch to the first available
        // and load its history
        if (wasCurrentSession) {
          const firstSessionId = sessions[0].sessionId
          import('@/lib/websocket').then(({ ws }) => {
            ws.setSessionId(firstSessionId)
          })
          // Load history for the first session asynchronously
          api.getSessionHistory(firstSessionId, { skipErrorToast: true }).then(historyResult => {
            const mergedMessages = mergeAssistantMessages(historyResult.messages || [])
            set((state) => {
              // Only update if we're still on the same session
              if (state.sessionId === firstSessionId) {
                return { messages: mergedMessages }
              }
              return {}
            })
          }).catch(err => {
            logError(err, { operation: 'Load history for first session' })
          })
          return {
            sessions,
            sessionId: firstSessionId,
            messages: [], // Will be populated by the async call above
          }
        }

        // Otherwise, just update the sessions list, keep current state
        return {
          ...state,
          sessions,
        }
      })
    } catch (error) {
      logError(error, { operation: 'Delete session' })
      throw error
    }
  },

  clearAllSessions: async () => {
    try {
      // IMPORTANT: Fetch ALL sessions from backend, not just from local state
      // This ensures we delete all sessions including those not currently loaded
      // (e.g., from tests, other browser windows, or previous runs)
      const allSessions = await api.listSessions(1, 1000) // Get up to 1000 sessions

      if (!Array.isArray(allSessions) || allSessions.length === 0) {
        return
      }

      const sessionIds = allSessions
        .map((s: any) => s.sessionId || s.id)
        .filter((id: string): id is string => id != null)

      // Bulk delete all sessions
      await api.bulkDeleteSessions(sessionIds)
    } catch (error) {
      logError(error, { operation: 'Bulk delete sessions' })
      throw error
    }

    // Clear local state temporarily
    set({
      sessions: [],
      sessionId: null,
      messages: [],
    })

    // Create a new session for continued use
    try {
      const result = await api.createSession()
      const newSession: ChatSession = {
        sessionId: result.sessionId,
        id: result.sessionId,
        createdAt: Date.now(),
        messageCount: 0,
      }

      // Update WebSocket
      const { ws } = await import('@/lib/websocket')
      ws.setSessionId(result.sessionId)

      // Set new state with empty messages (new session has no history)
      set({
        sessionId: result.sessionId,
        sessions: [newSession],
        messages: [],
      })
    } catch (createError) {
      logError(createError, { operation: 'Create new session after clearing' })
    }
  },

  updateSessionTitle: async (sessionId: string, title: string) => {
    try {
      await api.updateSession(sessionId, title)
      set((state) => ({
        sessions: state.sessions.map(s =>
          s.sessionId === sessionId ? { ...s, title } : s
        ),
      }))
    } catch (error) {
      logError(error, { operation: 'Update session title' })
      throw error
    }
  },

  loadSessions: async () => {
    if (!fetchCache.shouldFetch('sessions')) return
    fetchCache.markFetching('sessions')
    set({ sessionsLoading: true })
    try {
      // Reset pagination and load first page
      const pageSize = 10
      const result = await api.listSessions(1, pageSize)
      const sessions = normalizeSessionsResponse(result)

      // Check if there might be more sessions
      const hasMore = sessions.length >= pageSize

      set({
        sessions,
        sessionsPage: 1,
        sessionsHasMore: hasMore,
        sessionsLoading: false,
      })
      fetchCache.markFetched('sessions')
    } catch (error) {
      logError(error, { operation: 'Load sessions' })
      fetchCache.invalidate('sessions')
      set({ sessionsLoading: false })
    }
  },

  loadMoreSessions: async () => {
    const state = get()
    // Don't load if already loading or no more sessions
    if (state.sessionsLoading || !state.sessionsHasMore) {
      return
    }

    try {
      set({ sessionsLoading: true })

      const nextPage = state.sessionsPage + 1
      const pageSize = 10
      const result = await api.listSessions(nextPage, pageSize)
      const newSessions = normalizeSessionsResponse(result)

      // Check if there might be more sessions
      const hasMore = newSessions.length >= pageSize

      set((state) => ({
        sessions: [...state.sessions, ...newSessions],
        sessionsPage: nextPage,
        sessionsHasMore: hasMore,
        sessionsLoading: false,
      }))
    } catch (error) {
      logError(error, { operation: 'Load more sessions' })
      set({ sessionsLoading: false })
    }
  },

  fetchSessionHistory: async (sessionId: string) => {
    const cacheKey = `sessionHistory:${sessionId}`
    if (!fetchCache.shouldFetch(cacheKey)) return
    fetchCache.markFetching(cacheKey)
    try {
      const result = await api.getSessionHistory(sessionId)
      // Merge fragmented assistant messages from backend
      const mergedMessages = mergeAssistantMessages(result.messages || [])
      set({ messages: mergedMessages })
      fetchCache.markFetched(cacheKey)
    } catch (error) {
      logError(error, { operation: 'Fetch session history' })
      fetchCache.invalidate(cacheKey)
    }
  },
})
