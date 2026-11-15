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
