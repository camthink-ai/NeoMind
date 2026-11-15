import type { AuthorizeNewOperationRequest } from '@/types'

/** Structured Unknown re-run confirm emitted by the write gate / UI button. */
export interface AuthorizeNewOperationConfirm {
  requestId: string
  priorOperationId: string
  writeFingerprint: string
  validUntilMs: number
}

function kvToken(text: string, key: string): string | null {
  const re = new RegExp(`${key}=(\\S+)`)
  const match = text.match(re)
  const value = match?.[1]?.trim()
  return value || null
}

/**
 * Parse a structured confirm line
 * (`authorizeNewOperation confirm: request_id=… prior_operation_id=…
 * write_fingerprint=… valid_until_ms=…`).
 *
 * Incomplete / legacy `target:` markers without request id + validity are
 * rejected — the UI must not reconstruct a latest-B grant from body text.
 */
export function parseAuthorizeNewOperationConfirm(
  text: string,
): AuthorizeNewOperationConfirm | null {
  if (!text.includes('authorizeNewOperation confirm:')) return null
  const requestId = kvToken(text, 'request_id')
  const priorOperationId = kvToken(text, 'prior_operation_id')
  const writeFingerprint = kvToken(text, 'write_fingerprint')
  const validRaw = kvToken(text, 'valid_until_ms')
  if (!requestId || !priorOperationId || !writeFingerprint || !validRaw) return null
  const validUntilMs = Number(validRaw)
  if (!Number.isFinite(validUntilMs)) return null
  return { requestId, priorOperationId, writeFingerprint, validUntilMs }
}

/** @deprecated Prefer parseAuthorizeNewOperationConfirm — target-only is incomplete. */
export function parseAuthorizeNewOperationTarget(
  text: string,
): AuthorizeNewOperationRequest | null {
  const confirm = parseAuthorizeNewOperationConfirm(text)
  if (!confirm) return null
  return {
    priorOperationId: confirm.priorOperationId,
    writeFingerprint: confirm.writeFingerprint,
  }
}

export function isConfirmActive(
  confirm: AuthorizeNewOperationConfirm,
  nowMs: number = Date.now(),
): boolean {
  return confirm.validUntilMs > nowMs
}

export function toAuthorizeRequest(
  confirm: AuthorizeNewOperationConfirm,
): AuthorizeNewOperationRequest {
  return {
    priorOperationId: confirm.priorOperationId,
    writeFingerprint: confirm.writeFingerprint,
  }
}

/** Collect text blobs that may carry structured confirm lines. */
export function messageTextsForConfirmScan(
  messages: Array<{ content?: string; tool_calls?: Array<{ result?: unknown }> }>,
): string[] {
  const out: string[] = []
  for (const message of messages) {
    if (message.content) out.push(message.content)
    for (const tc of message.tool_calls ?? []) {
      if (typeof tc.result === 'string') out.push(tc.result)
      else if (tc.result != null) {
        try {
          out.push(JSON.stringify(tc.result))
        } catch {
          // ignore non-serializable tool results
        }
      }
    }
  }
  return out
}

/**
 * Active pending confirms from structured sources only.
 * Dedupes by requestId; expired entries are dropped.
 */
export function collectActivePendingConfirms(
  sources: Iterable<string>,
  nowMs: number = Date.now(),
): AuthorizeNewOperationConfirm[] {
  const byId = new Map<string, AuthorizeNewOperationConfirm>()
  for (const text of sources) {
    const parsed = parseAuthorizeNewOperationConfirm(text)
    if (!parsed || !isConfirmActive(parsed, nowMs)) continue
    byId.set(parsed.requestId, parsed)
  }
  return [...byId.values()]
}

/** Phrases that map to AuthorizeNewOperation when exactly one confirm is active. */
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
 * Phrase shortcut: attach auth only when there is exactly one active pending
 * confirm. Multi-candidate → null (user must pick an explicit op button).
 * Never scans history bodies for a latest parseable target.
 */
export function resolveAuthorizeNewOperationForSend(
  userText: string,
  pendingConfirms: AuthorizeNewOperationConfirm[],
  nowMs: number = Date.now(),
): AuthorizeNewOperationRequest | null {
  if (!isAuthorizeNewOperationPhrase(userText)) return null
  const active = pendingConfirms.filter((c) => isConfirmActive(c, nowMs))
  if (active.length !== 1) return null
  return toAuthorizeRequest(active[0])
}
