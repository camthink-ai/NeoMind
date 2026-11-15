import type { AuthorizeNewOperationRequest } from '@/types'

/**
 * Parse `priorOperationId` + `writeFingerprint` from a gate confirm-request
 * body (`authorizeNewOperation target: prior_operation_id=… write_fingerprint=…`).
 * Used when the UI echoes the blocked Unknown op back on the next chat turn.
 */
export function parseAuthorizeNewOperationTarget(
  text: string,
): AuthorizeNewOperationRequest | null {
  const idMatch = text.match(/prior_operation_id=(\S+)/)
  const fpMatch = text.match(/write_fingerprint=(\S+)/)
  const priorOperationId = idMatch?.[1]?.trim()
  const writeFingerprint = fpMatch?.[1]?.trim()
  if (!priorOperationId || !writeFingerprint) return null
  return { priorOperationId, writeFingerprint }
}

/** Phrases that map to AuthorizeNewOperation on the server (unique-Unknown shortcut). */
export function isAuthorizeNewOperationPhrase(text: string): boolean {
  const t = text.trim().toLowerCase()
  return (
    t === '确认重跑' ||
    t === 'confirm re-run' ||
    t === 'confirm rerun' ||
    t === 'authorize new operation'
  )
}

/**
 * Confirm-op loop: when the user sends a confirm phrase, attach the latest
 * parseable target from recent assistant/tool text so multi-Unknown grants
 * travel with the message (not a shared hop).
 */
export function resolveAuthorizeNewOperationForSend(
  userText: string,
  recentTexts: Iterable<string>,
): AuthorizeNewOperationRequest | null {
  if (!isAuthorizeNewOperationPhrase(userText)) return null
  let found: AuthorizeNewOperationRequest | null = null
  for (const text of recentTexts) {
    const parsed = parseAuthorizeNewOperationTarget(text)
    if (parsed) found = parsed
  }
  return found
}
