/**
 * Session Slice
 *
 * Handles chat session management and message history.
 * Supports multiple sessions with switching capability.
 */

import type { StateCreator } from 'zustand'
import type { Message } from '@/types'

/**
 * Merge fragmented assistant messages from backend.
 *
 * The backend may split assistant responses into multiple messages:
 * - First message: thinking + tool_calls (without content)
 * - Second message: content only
 * - Sometimes: multiple content chunks that should be combined
 *
 * This function merges them back into a single message for display.
 * Also filters out internal tool role messages.
 */
function mergeAssistantMessages(messages: Message[]): Message[] {
  const result: Message[] = []

  for (let i = 0; i < messages.length; i++) {
    const msg = messages[i]

    // Skip tool role messages (internal LLM context)
    if (isToolMessage(msg)) {
      continue
    }

    // Check if this is an assistant message that might need merging with the next one
    if (msg.role === 'assistant' && i + 1 < messages.length) {
      const nextMsg = messages[i + 1]

      // Skip if next message is a tool message
      if (isToolMessage(nextMsg)) {
        result.push(msg)
        continue
      }

      // Check if next message is also an assistant message
      if (nextMsg.role === 'assistant') {
        // Merge logic: if they should be merged based on content structure
        if (shouldMergeMessages(msg, nextMsg)) {
          // Merge the two messages - combine content without duplicating when backend sent same content twice
          const mergedContent = dedupeContentConcat(msg.content || '', nextMsg.content || '') || ''
          result.push({
            ...msg,
            content: mergedContent,
            // Keep tool_calls, thinking from the first message (it usually has them)
            tool_calls: msg.tool_calls || nextMsg.tool_calls,
            thinking: msg.thinking || nextMsg.thinking,
            // Use the earlier timestamp
            timestamp: msg.timestamp,
            id: msg.id, // Keep the first message's ID
          })
          // Skip the next message since we merged it
          i++
          continue
        } else if (msg.content && !msg.tool_calls && !msg.thinking) {
          // Special case: first message is plain content without tools/thinking
          // If next message has tools/thinking, this is a split response - merge them
          if (nextMsg.tool_calls || nextMsg.thinking) {
            const mergedContent = dedupeContentConcat(msg.content || '', nextMsg.content || '') || ''
            result.push({
              ...msg,
              content: mergedContent,
              tool_calls: nextMsg.tool_calls,
              thinking: nextMsg.thinking,
              timestamp: msg.timestamp,
              id: msg.id,
            })
            i++
            continue
          }
        }
      }
    }

    result.push(msg)
  }

  return result
}

/**
 * Combine two content strings without duplicating when backend sent the same content twice
 * (e.g. thinking+tools+content in first message and content-only in second with same text).
 */
function dedupeContentConcat(a: string, b: string): string {
  const x = (a || '').trim()
  const y = (b || '').trim()
  if (!y) return a || ''
  if (!x) return b || ''
  if (x === y) return a
  if (x.endsWith(y)) return a
  if (y.startsWith(x)) return b
  if (x.includes(y)) return a
  return (a || '') + (b || '')
}

/**
 * Check if two assistant messages should be merged.
 *
 * They should be merged if:
 * - First has thinking OR tools
 * - Second has content OR first is missing content (split response)
 * - They are consecutive assistant messages
 *
 * Backend pattern: [thinking+tools] + [content] or [thinking] + [tools+content]
 */
function shouldMergeMessages(first: Message, second: Message): boolean {
  const firstHasThinking = !!first.thinking && first.thinking.length > 0
  const firstHasTools = !!first.tool_calls && first.tool_calls.length > 0
  const firstHasContent = !!first.content && first.content.length > 0

  const secondHasThinking = !!second.thinking && second.thinking.length > 0
  const secondHasTools = !!second.tool_calls && second.tool_calls.length > 0
  const secondHasContent = !!second.content && second.content.length > 0

  // Always merge consecutive assistant messages where first has thinking or tools
  // This handles the backend pattern of splitting responses
  if (firstHasThinking || firstHasTools) {
    // Merge if second has content, OR if first is missing content (split response)
    return !firstHasContent || secondHasContent
  }

  // Also merge if second has thinking or tools and first only has content
  if ((secondHasThinking || secondHasTools) && firstHasContent) {
    return true
  }

  return false
}

import type { SessionState } from '../types'
import type { ChatSession } from '@/types'
import { api } from '@/lib/api'
import { normalizeSessions, normalizeSessionsResponse } from '@/lib/api/transforms'
import { isToolMessage } from '@/types'

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
> = (set, get) => ({
  // Initial state
  sessionId: null,
  messages: [],
  sessions: [],

  // Actions
  setSessionId: (id: string) => {
    set({ sessionId: id })
  },

  addMessage: (message: Message) => {
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

      // Update current session in sessions list
      // This ensures the session list shows updated preview and message count
      const updatedSessions = state.sessions.map(s => {
        if (s.sessionId === state.sessionId) {
          // For user messages, use as preview; for assistant, keep existing or use first 50 chars
          const preview = message.role === 'user'
            ? message.content
            : (s.preview || (message.content ? message.content.slice(0, 50) + '...' : ''))

          return {
            ...s,
            preview,
            messageCount: (s.messageCount || 0) + 1,
            updatedAt: Date.now(),
          }
        }
        return s
      })

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
    try {
      const result = await api.createSession()

      // Reload sessions from server to ensure consistency
      // This avoids issues where local state diverges from server state
      const listResult = await api.listSessions()
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
      console.error('Failed to create session:', error)
      return null
    }
  },

  switchSession: async (sessionId: string) => {
    // Check if we're already on this session to avoid unnecessary API calls
    const currentSessionId = get().sessionId
    if (sessionId === currentSessionId) {
      return
    }

    // IMPORTANT: Update WebSocket FIRST before any API calls
    // This ensures any subsequent messages go to the correct session
    const { ws } = await import('@/lib/websocket')
    ws.setSessionId(sessionId)

    try {
      // Fetch the session history
      const historyResult = await api.getSessionHistory(sessionId)

      // Validate the response before processing
      if (!historyResult) {
        console.warn(`Session ${sessionId} returned empty result from API`)
        set({ sessionId, messages: [] })
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
      })
    } catch (error: any) {
      console.error('Failed to switch session:', error)

      // If session not found (404), show error but don't auto-delete
      // This prevents data loss when database has inconsistencies
      if (error?.error?.code === 'NOT_FOUND' || error?.status === 404) {
        console.warn(`Session ${sessionId} not found in database. This may indicate data corruption.`)

        // Try to reload sessions from server to get accurate list
        try {
          const result = await api.listSessions()
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
            }))
          } else {
            // No sessions on server - keep local state and notify user
            console.error('No sessions found on server. Database may be corrupted.')
          }
        } catch (loadError) {
          console.error('Failed to reload sessions:', loadError)
        }
        return
      }

      // For other errors, just keep current state
      console.error('Error switching session:', error)
    }
  },

  deleteSession: async (sessionIdToDelete: string) => {
    try {
      await api.deleteSession(sessionIdToDelete)

      // After successful deletion, reload sessions from server
      // This ensures consistency between server and client state
      const result = await api.listSessions()
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
          api.getSessionHistory(firstSessionId).then(historyResult => {
            const mergedMessages = mergeAssistantMessages(historyResult.messages || [])
            set((state) => {
              // Only update if we're still on the same session
              if (state.sessionId === firstSessionId) {
                return { messages: mergedMessages }
              }
              return {}
            })
          }).catch(err => {
            console.error('Failed to load history for first session:', err)
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
      console.error('Failed to delete session:', error)
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
      console.error('Failed to bulk delete sessions:', error)
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
      const sessions = normalizeSessionsResponse(result)

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
