/**
 * API Data Transformation Utilities
 *
 * Normalizes data from backend to frontend format.
 * Handles snake_case to camelCase conversion and missing fields.
 */

import type { ChatSession } from '@/types'

/**
 * Raw session data from backend (snake_case or camelCase)
 */
interface RawSession {
  sessionId?: string
  id?: string
  session_id?: string
  createdAt?: number
  created_at?: number
  updatedAt?: number
  updated_at?: number
  messageCount?: number
  message_count?: number
  title?: string | null
  preview?: string | null
  [key: string]: unknown
}

/**
 * Normalize a single session from backend format to frontend format.
 *
 * Handles:
 * - snake_case to camelCase field names
 * - Missing fields with sensible defaults
 * - Type coercion
 *
 * @param raw - Raw session data from API
 * @returns Normalized ChatSession object
 */
export function normalizeSession(raw: unknown): ChatSession | null {
  if (!raw || typeof raw !== 'object') {
    return null
  }

  const s = raw as RawSession

  // Extract session ID (try multiple field names)
  const sessionId = s.sessionId || s.id || s.session_id
  if (!sessionId) {
    return null
  }

  return {
    sessionId,
    id: sessionId,
    createdAt: s.createdAt || s.created_at || Date.now(),
    updatedAt: s.updatedAt ?? s.updated_at ?? undefined,
    messageCount: s.messageCount || s.message_count || 0,
    title: s.title ?? undefined,
    preview: s.preview ?? undefined,
  }
}

/**
 * Normalize an array of sessions from backend format.
 *
 * Filters out invalid sessions and applies normalization to each valid one.
 *
 * @param rawArray - Array of raw session data from API
 * @returns Array of normalized ChatSession objects
 */
export function normalizeSessions(rawArray: unknown[]): ChatSession[] {
  if (!Array.isArray(rawArray)) {
    return []
  }

  return rawArray
    .map((item) => normalizeSession(item))
    .filter((session): session is ChatSession => session !== null)
}

/**
 * Normalize API response that may be wrapped or direct array.
 *
 * Handles both formats:
 * - Direct array: [{ ...session }, { ...session }]
 * - Wrapped: { sessions: [...], count: N }
 *
 * @param response - Raw API response
 * @returns Array of normalized ChatSession objects
 */
export function normalizeSessionsResponse(response: unknown): ChatSession[] {
  // If response is an array, normalize directly
  if (Array.isArray(response)) {
    return normalizeSessions(response)
  }

  // If response is an object, try to extract the array
  if (response && typeof response === 'object') {
    const obj = response as Record<string, unknown>

    // Try common field names
    const sessionsArray = obj.sessions || obj.data || obj.results || obj.items

    if (Array.isArray(sessionsArray)) {
      return normalizeSessions(sessionsArray)
    }
  }

  return []
}
