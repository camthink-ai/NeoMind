import type { ChatSession } from '@/types'

/** Placeholder titles the server / create flow may return before a real name. */
const PLACEHOLDER_TITLES = new Set(['新对话', 'New Chat'])

/**
 * Display title for a chat session.
 * Prefer a real `title`, then first-message `preview`, then the i18n default.
 */
export function getSessionDisplayTitle(
  session: ChatSession | undefined,
  fallback: string,
): string {
  if (!session) return fallback
  const title = session.title?.trim()
  if (title && !PLACEHOLDER_TITLES.has(title)) return title
  const preview = session.preview?.trim()
  if (preview) {
    return preview.length > 30 ? `${preview.slice(0, 30)}…` : preview
  }
  return fallback
}
