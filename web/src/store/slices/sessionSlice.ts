/**
 * Session Slice
 *
 * Handles chat session management and message history.
 * Supports multiple sessions with switching capability.
 */

import type { StateCreator } from 'zustand'
import type { SessionState } from '../types'
import type { Message, ChatSession } from '@/types'
import { api } from '@/lib/api'

export interface SessionSlice extends SessionState {
  // Actions
  setSessionId: (id: string) => void
  addMessage: (message: Message) => void
  clearMessages: () => void
  createSession: () => Promise<string | null>
  switchSession: (sessionId: string) => Promise<void>
  deleteSession: (sessionId: string) => Promise<void>
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
      const newSession: ChatSession = {
        sessionId: result.sessionId,
        id: result.sessionId,
        createdAt: Date.now(),
      }
      set((state) => ({
        sessionId: result.sessionId,
        messages: [],
        sessions: [newSession, ...state.sessions],
      }))

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
      set({
        sessionId,
        messages: historyResult.messages || [],
      })

      // Update WebSocket to use the new session
      const { ws } = await import('@/lib/websocket')
      ws.setSessionId(sessionId)

      // Send session switch message via WebSocket
      ws.sendMessage(`/switch:${sessionId}`)
    } catch (error) {
      console.error('Failed to switch session:', error)
      // If no history, start fresh
      set({
        sessionId,
        messages: [],
      })
    }
  },

  deleteSession: async (sessionId: string) => {
    try {
      await api.deleteSession(sessionId)
      set((state) => {
        const filtered = state.sessions.filter(s => s.sessionId !== sessionId)
        // If deleting current session, switch to the first available or null
        const newSessionId = state.sessionId === sessionId
          ? (filtered.length > 0 ? filtered[0].sessionId : null)
          : state.sessionId
        return {
          sessions: filtered,
          sessionId: newSessionId,
          messages: newSessionId ? [] : state.messages, // Clear messages if switched
        }
      })
    } catch (error) {
      console.error('Failed to delete session:', error)
    }
  },

  loadSessions: async () => {
    try {
      const result = await api.listSessions()
      const sessions: ChatSession[] = result.sessions.map((s: any) => ({
        sessionId: s.sessionId || s.id,
        id: s.sessionId || s.id,
        createdAt: s.createdAt || s.created_at || Date.now(),
        updatedAt: s.updatedAt || s.updated_at,
        messageCount: s.messageCount || s.message_count,
        preview: s.preview,
      }))
      set({ sessions })
    } catch (error) {
      console.error('Failed to load sessions:', error)
    }
  },

  fetchSessionHistory: async (sessionId: string) => {
    try {
      const result = await api.getSessionHistory(sessionId)
      set({ messages: result.messages || [] })
    } catch (error) {
      console.error('Failed to fetch session history:', error)
    }
  },
})
