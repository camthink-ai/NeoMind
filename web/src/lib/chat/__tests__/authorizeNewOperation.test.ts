import { describe, it, expect } from 'vitest'
import { parseAuthorizeNewOperationTarget } from '../authorizeNewOperation'

describe('parseAuthorizeNewOperationTarget', () => {
  it('parses prior_operation_id + write_fingerprint from confirm request', () => {
    const text =
      'Write blocked: operation op-abc ended with unknown outcome.\n' +
      'authorizeNewOperation target: prior_operation_id=op-abc ' +
      'write_fingerprint=run_now|{"agent_id":"A"}'
    expect(parseAuthorizeNewOperationTarget(text)).toEqual({
      priorOperationId: 'op-abc',
      writeFingerprint: 'run_now|{"agent_id":"A"}',
    })
  })

  it('returns null when fields are missing', () => {
    expect(parseAuthorizeNewOperationTarget('confirm re-run')).toBeNull()
  })
})
