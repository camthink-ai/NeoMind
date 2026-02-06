/**
 * Session Selectors
 *
 * Memoized selectors for session state using Zustand's optimized selector pattern.
 */

import type { ChatSession } from '@/types'
import type { NeoMindStore } from '../'

// ============================================================================
// Base Selectors
// ============================================================================

/**
 * Get all sessions
 */
export const selectSessionsRaw = (state: NeoMindStore): ChatSession[] => state.sessions

// ============================================================================
// Active Session Selectors
// ============================================================================

/**
 * Get the currently active session
 */
export const selectActiveSession = (state: NeoMindStore): ChatSession | undefined =>
  state.sessions.find((s) => s.id === state.sessionId)

/**
 * Get active session ID
 */
export const selectActiveSessionId = (state: NeoMindStore): string | null =>
  state.sessionId

// ============================================================================
// Session Lookup Selectors
// ============================================================================

/**
 * Get session by ID
 */
export const selectSessionById = (state: NeoMindStore, sessionId: string): ChatSession | undefined =>
  state.sessions.find((s) => s.id === sessionId)

/**
 * Get sessions as a Map for O(1) lookups
 */
export const selectSessionMap = (state: NeoMindStore): Map<string, ChatSession> => {
  const map = new Map<string, ChatSession>()
  state.sessions.forEach(s => map.set(s.id, s))
  return map
}

// ============================================================================
// Sorted Session Selectors
// ============================================================================

/**
 * Get sessions sorted by last activity (most recent first)
 */
export const selectSessionsByRecent = (state: NeoMindStore): ChatSession[] => {
  return [...state.sessions].sort((a, b) => {
    const aTime = a.updatedAt ?? a.createdAt
    const bTime = b.updatedAt ?? b.createdAt
    return bTime - aTime
  })
}

/**
 * Get sessions sorted by creation date (newest first)
 */
export const selectSessionsByNewest = (state: NeoMindStore): ChatSession[] => {
  return [...state.sessions].sort((a, b) => b.createdAt - a.createdAt)
}

/**
 * Get sessions sorted by name (alphabetical)
 */
export const selectSessionsByName = (state: NeoMindStore): ChatSession[] => {
  return [...state.sessions].sort((a, b) =>
    (a.title ?? 'Untitled').localeCompare(b.title ?? 'Untitled')
  )
}

// ============================================================================
// Session Summary Selectors
// ============================================================================

/**
 * Get session count
 */
export const selectSessionCount = (state: NeoMindStore): number =>
  state.sessions.length

/**
 * Get sessions grouped by date (today, this week, this month, older)
 */
export const selectSessionsGroupedByDate = (state: NeoMindStore) => {
  const now = Date.now()
  const today = now - (24 * 60 * 60 * 1000)
  const thisWeek = now - (7 * 24 * 60 * 60 * 1000)
  const thisMonth = now - (30 * 24 * 60 * 60 * 1000)

  const groups = {
    today: [] as ChatSession[],
    thisWeek: [] as ChatSession[],
    thisMonth: [] as ChatSession[],
    older: [] as ChatSession[],
  }

  for (const session of state.sessions) {
    const time = session.updatedAt ?? session.createdAt
    if (time >= today) {
      groups.today.push(session)
    } else if (time >= thisWeek) {
      groups.thisWeek.push(session)
    } else if (time >= thisMonth) {
      groups.thisMonth.push(session)
    } else {
      groups.older.push(session)
    }
  }

  return groups
}

/**
 * Get session summary statistics
 */
export const selectSessionsSummary = (state: NeoMindStore) => {
  const totalMessages = state.sessions.reduce((sum, s) => sum + (s.messageCount ?? 0), 0)
  const avgMessagesPerSession = state.sessions.length > 0
    ? totalMessages / state.sessions.length
    : 0

  return {
    total: state.sessions.length,
    totalMessages,
    avgMessagesPerSession: Math.round(avgMessagesPerSession * 100) / 100,
    hasActiveSession: !!state.sessionId,
  }
}

// ============================================================================
// Session Filter Selectors
// ============================================================================

/**
 * Get sessions containing a search term in title or preview
 */
export const selectSessionsBySearchTerm = (state: NeoMindStore, searchTerm: string): ChatSession[] => {
  if (!searchTerm.trim()) {
    return state.sessions
  }

  const term = searchTerm.toLowerCase()
  return state.sessions.filter((s) => {
    const nameMatch = (s.title ?? '').toLowerCase().includes(term)
    const previewMatch = (s.preview ?? '').toLowerCase().includes(term)
    return nameMatch || previewMatch
  })
}

/**
 * Get sessions with message count filter
 */
export const selectSessionsWithMessages = (state: NeoMindStore, minMessages: number = 1): ChatSession[] =>
  state.sessions.filter((s) => (s.messageCount ?? 0) >= minMessages)
