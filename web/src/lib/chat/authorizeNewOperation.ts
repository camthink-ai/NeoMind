import type { AuthorizeNewOperationRequest } from '@/types'

/**
 * Structured Unknown re-run confirm from server WS events
 * (`AuthorizeConfirmRequest`). Fingerprints travel as JSON fields and may
 * contain spaces (e.g. `shell|neomind device control …`) — never parse them
 * out of prose with `(\S+)` / split_whitespace.
 */
export interface AuthorizeNewOperationConfirm {
  requestId: string
  priorOperationId: string
  writeFingerprint: string
  toolName?: string
  target?: string | null
  validUntilMs: number
}

/** Build the host submit payload (request-id keyed). */
export function toAuthorizeRequest(
  confirm: AuthorizeNewOperationConfirm,
): AuthorizeNewOperationRequest {
  return {
    requestId: confirm.requestId,
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

/** Upsert a confirm from a WS AuthorizeConfirmRequest event. */
export function upsertPendingConfirm(
  pending: AuthorizeNewOperationConfirm[],
  incoming: AuthorizeNewOperationConfirm,
  nowMs: number = Date.now(),
): AuthorizeNewOperationConfirm[] {
  if (!isConfirmActive(incoming, nowMs)) {
    return pending.filter((c) => c.requestId !== incoming.requestId)
  }
  const next = pending.filter((c) => c.requestId !== incoming.requestId)
  next.push(incoming)
  return next
}

/** Remove a confirm (consumed / expired / cancelled). */
export function removePendingConfirm(
  pending: AuthorizeNewOperationConfirm[],
  requestId: string,
): AuthorizeNewOperationConfirm[] {
  return pending.filter((c) => c.requestId !== requestId)
}

/** Drop wall-clock-expired entries. */
export function filterActivePendingConfirms(
  pending: AuthorizeNewOperationConfirm[],
  nowMs: number = Date.now(),
): AuthorizeNewOperationConfirm[] {
  return pending.filter((c) => isConfirmActive(c, nowMs))
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
 * Pending list comes from WS events — never from history-body text scan.
 */
export function resolveAuthorizeNewOperationForSend(
  userText: string,
  pendingConfirms: AuthorizeNewOperationConfirm[],
  nowMs: number = Date.now(),
): AuthorizeNewOperationRequest | null {
  if (!isAuthorizeNewOperationPhrase(userText)) return null
  const active = filterActivePendingConfirms(pendingConfirms, nowMs)
  if (active.length !== 1) return null
  return toAuthorizeRequest(active[0])
}

/** Display label: op short id + optional target name. */
export function confirmDisplayLabel(confirm: AuthorizeNewOperationConfirm): {
  opId: string
  target?: string
} {
  const id = confirm.priorOperationId
  const opId = id.length <= 12 ? id : `${id.slice(0, 8)}…`
  const target = confirm.target?.trim() || undefined
  return { opId, target }
}
