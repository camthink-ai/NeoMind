/**
 * Session Slice
 *
 * Handles chat session management and message history.
 * Supports multiple sessions with switching capability.
 */

import type { StateCreator } from 'zustand'
import type { Message } from '@/types'

/**
 * Merge fragmented assistant messages from backend storage.
 *
 * Backend stores messages as separate parts:
 * - [assistant_msg1] with thinking + tool_calls (but empty/minimal content)
 * - [tool_result_msgs] with role="tool"
 * - [assistant_msg2] with content only (no thinking/tool_calls)
 *
 * This function merges them into the format expected by frontend:
 * - [assistant_msg] with thinking + tool_calls + content
 */
function mergeAssistantMessages(messages: Message[]): Message[] {
  const result: Message[] = []
  let pendingAssistantMessage: Message | null = null

  for (const msg of messages) {
    // Skip tool role messages - they're just for LLM context
    // Use type assertion since backend sends 'tool' role but frontend type doesn't include it
    if ((msg as any).role === 'tool') {
      continue
    }

    if (msg.role === 'assistant') {
      // Check if this message has content but no thinking/tool_calls
      const hasOnlyContent = msg.content &&
        msg.content.trim().length > 0 &&
        !msg.thinking &&
        (!msg.tool_calls || msg.tool_calls.length === 0)

      // Check if this message has thinking/tool_calls but minimal content
      const hasStructureOnly = (msg.thinking || (msg.tool_calls && msg.tool_calls.length > 0)) &&
        (!msg.content || msg.content.trim().length === 0)

      if (hasOnlyContent && pendingAssistantMessage) {
        // This is the content part - merge with pending message
        pendingAssistantMessage.content = msg.content
        pendingAssistantMessage.timestamp = msg.timestamp // Use the timestamp of when content was generated
        result.push(pendingAssistantMessage)
        pendingAssistantMessage = null
      } else if (hasStructureOnly) {
        // This is the structure part (thinking + tools) - hold for next content message
        pendingAssistantMessage = { ...msg }
      } else {
        // Complete standalone message - just add it
        if (pendingAssistantMessage) {
          result.push(pendingAssistantMessage)
          pendingAssistantMessage = null
        }
        result.push(msg)
      }
    } else {
      // User or system message
      if (pendingAssistantMessage) {
        result.push(pendingAssistantMessage)
        pendingAssistantMessage = null
      }
      result.push(msg)
    }
  }

  // Don't forget any pending message
  if (pendingAssistantMessage) {
    result.push(pendingAssistantMessage)
  }

  return result
}

import type { SessionState } from '../types'
import type { ChatSession } from '@/types'
import { api } from '@/lib/api'

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
  fetchSessionHistory: (sessionId: string) => Promise<void>
}

export const createSessionSlice: StateCreator<
  SessionSlice,
  [],
  [],
  SessionSlice
> = (set) => ({
  // Initial state
  sessionId: null,
  messages: [],
  sessions: [],

  // Actions
  setSessionId: (id: string) => {
    set({ sessionId: id })
  },

  addMessage: (message: Message) => {
    set((state) => ({
      messages: [...state.messages, message],
    }))
  },

  clearMessages: () => {
    set({ messages: [] })
  },

  createSession: async () => {
    try {
      const result = await api.createSession()

      // Reload sessions from server to ensure consistency
      // This avoids issues where local state diverges from server state
      const listResult = await api.listSessions()
      const sessionsArray = Array.isArray(listResult) ? listResult : (listResult as any).sessions || []
      const sessions: ChatSession[] = sessionsArray.map((s: any) => ({
        sessionId: s.sessionId || s.id,
        id: s.sessionId || s.id,
        createdAt: s.createdAt || s.created_at || Date.now(),
        updatedAt: s.updatedAt || s.updated_at,
        messageCount: s.messageCount || s.message_count || 0,
        title: s.title ?? undefined,
        preview: s.preview,
      }))

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
      console.error('Failed to create session:', error)
      return null
    }
  },

  switchSession: async (sessionId: string) => {
    try {
      // Fetch the session history
      const historyResult = await api.getSessionHistory(sessionId)

      // Debug: Check if backend returns tool call results
      const assistantMessages = (historyResult.messages || []).filter((m: any) => m.role === 'assistant' && m.tool_calls?.length > 0)
      if (assistantMessages.length > 0) {
        const toolsWithResults = assistantMessages[0].tool_calls?.filter((tc: any) => tc.result !== undefined && tc.result !== null).length || 0
        console.log(`[sessionSlice] Found ${assistantMessages.length} assistant messages with tool calls, ${toolsWithResults} have results`)
        if (toolsWithResults === 0) {
          console.warn('[sessionSlice] No tool calls have results! Backend may be using old code.')
          console.log('[sessionSlice] Tool call data:', assistantMessages[0].tool_calls)
        }
      }

      // Merge fragmented assistant messages from backend
      // Backend stores: [msg1(thinking+tools)] + [tool results] + [msg2(content only)]
      // Frontend expects: [msg1(thinking+tools+content)]
      const mergedMessages = mergeAssistantMessages(historyResult.messages || [])

      set({
        sessionId,
        messages: mergedMessages,
      })

      // Update WebSocket to use the new session
      const { ws } = await import('@/lib/websocket')
      ws.setSessionId(sessionId)
    } catch (error: any) {
      console.error('Failed to switch session:', error)

      // If session not found (404), remove it from the list
      if (error?.error?.code === 'NOT_FOUND' || error?.status === 404) {
        // Remove the invalid session from list
        set((state) => ({
          sessions: state.sessions.filter(s => s.sessionId !== sessionId),
        }))

        // Try to load sessions from server to get fresh list
        try {
          const result = await api.listSessions()
          const sessionsArray = Array.isArray(result) ? result : (result as any).sessions || []
          const sessions: ChatSession[] = sessionsArray.map((s: any) => ({
            sessionId: s.sessionId || s.id,
            id: s.sessionId || s.id,
            createdAt: s.createdAt || s.created_at || Date.now(),
            updatedAt: s.updatedAt || s.updated_at,
            messageCount: s.messageCount || s.message_count || 0,
            title: s.title ?? undefined,
            preview: s.preview,
          }))

          set((state) => ({
            sessions,
            // If we still have sessions, switch to the first one
            sessionId: sessions.length > 0 ? sessions[0].sessionId : null,
            messages: sessions.length > 0 ? [] : state.messages,
          }))

          // Update WebSocket if we switched sessions
          if (sessions.length > 0 && sessions[0].sessionId) {
            const { ws } = await import('@/lib/websocket')
            ws.setSessionId(sessions[0].sessionId)
          }

          // If no sessions left, create a new one
          if (sessions.length === 0) {
            const createResult = await api.createSession()
            const newSession: ChatSession = {
              sessionId: createResult.sessionId,
              id: createResult.sessionId,
              createdAt: Date.now(),
            }
            set({
              sessionId: createResult.sessionId,
              messages: [],
              sessions: [newSession],
            })

            const { ws } = await import('@/lib/websocket')
            ws.setSessionId(createResult.sessionId)
          }
        } catch (loadError) {
          console.error('Failed to reload sessions:', loadError)
        }
        return
      }

      // For other errors, just clear messages and set the session ID
      set({
        sessionId,
        messages: [],
      })
    }
  },

  deleteSession: async (sessionIdToDelete: string) => {
    try {
      await api.deleteSession(sessionIdToDelete)

      // After successful deletion, reload sessions from server
      // This ensures consistency between server and client state
      const result = await api.listSessions()
      const sessionsArray = Array.isArray(result) ? result : (result as any).sessions || []
      const sessions: ChatSession[] = sessionsArray.map((s: any) => ({
        sessionId: s.sessionId || s.id,
        id: s.sessionId || s.id,
        createdAt: s.createdAt || s.created_at || Date.now(),
        updatedAt: s.updatedAt || s.updated_at,
        messageCount: s.messageCount || s.message_count || 0,
        title: s.title ?? undefined,
        preview: s.preview,
      }))

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
        if (wasCurrentSession) {
          const firstSessionId = sessions[0].sessionId
          import('@/lib/websocket').then(({ ws }) => {
            ws.setSessionId(firstSessionId)
          })
          return {
            sessions,
            sessionId: firstSessionId,
            messages: [],
          }
        }

        // Otherwise, just update the sessions list, keep current state
        return {
          ...state,
          sessions,
        }
      })
    } catch (error) {
      console.error('Failed to delete session:', error)
      throw error
    }
  },

  clearAllSessions: async () => {
    // First get sessions from current state before clearing
    let sessionIds: string[] = []
    set((state) => {
      sessionIds = state.sessions.map(s => s.sessionId).filter((id): id is string => id != null)
      return state // Just reading, no change yet
    })

    if (sessionIds.length === 0) {
      return
    }

    try {
      // Bulk delete all sessions
      await api.bulkDeleteSessions(sessionIds)
    } catch (error) {
      console.error('Failed to bulk delete sessions:', error)
      throw error
    }

    // Clear local state
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
      }
      set({
        sessionId: result.sessionId,
        sessions: [newSession],
      })

      // Update WebSocket
      const { ws } = await import('@/lib/websocket')
      ws.setSessionId(result.sessionId)
    } catch (createError) {
      console.error('Failed to create new session after clearing:', createError)
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
      console.error('Failed to update session title:', error)
      throw error
    }
  },

  loadSessions: async () => {
    try {
      const result = await api.listSessions()
      // Backend returns an array directly (after ApiResponse auto-unwrap)
      const sessionsArray = Array.isArray(result) ? result : (result as any).sessions || []
      const sessions: ChatSession[] = sessionsArray.map((s: any) => ({
        sessionId: s.sessionId || s.id,
        id: s.sessionId || s.id,
        createdAt: s.createdAt || s.created_at || Date.now(),
        updatedAt: s.updatedAt || s.updated_at,
        messageCount: s.messageCount || s.message_count || 0,
        title: s.title ?? undefined,
        preview: s.preview,
      }))

      // Only update sessions list, preserve sessionId and messages
      // This prevents accidental session switching or message loss
      set((state) => ({
        ...state,
        sessions,
      }))
    } catch (error) {
      console.error('Failed to load sessions:', error)
    }
  },

  fetchSessionHistory: async (sessionId: string) => {
    try {
      const result = await api.getSessionHistory(sessionId)
      // Merge fragmented assistant messages from backend
      const mergedMessages = mergeAssistantMessages(result.messages || [])
      set({ messages: mergedMessages })
    } catch (error) {
      console.error('Failed to fetch session history:', error)
    }
  },
})
