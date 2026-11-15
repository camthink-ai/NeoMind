import {
  isAuthorizeNewOperationPhrase,
  parseAuthorizeNewOperationTarget,
  resolveAuthorizeNewOperationForSend,
} from '../authorizeNewOperation'

describe('parseAuthorizeNewOperationTarget', () => {
  it('parses prior_operation_id + write_fingerprint from confirm request', () => {
    const text =
      'authorizeNewOperation target: prior_operation_id=op-abc ' +
      'write_fingerprint=run_now|{"agent_id":"A"}'
    expect(parseAuthorizeNewOperationTarget(text)).toEqual({
      priorOperationId: 'op-abc',
      writeFingerprint: 'run_now|{"agent_id":"A"}',
    })
  })

  it('returns null when target markers are absent', () => {
    expect(parseAuthorizeNewOperationTarget('confirm re-run')).toBeNull()
  })
})

describe('resolveAuthorizeNewOperationForSend', () => {
  const confirmBody =
    'Write blocked… authorizeNewOperation target: prior_operation_id=op-a ' +
    'write_fingerprint=fp-a'

  it('attaches latest target when user sends a confirm phrase', () => {
    expect(
      resolveAuthorizeNewOperationForSend('确认重跑', [
        'unrelated',
        confirmBody,
        'later unrelated',
      ]),
    ).toEqual({ priorOperationId: 'op-a', writeFingerprint: 'fp-a' })
  })

  it('does not attach on ordinary messages', () => {
    expect(
      resolveAuthorizeNewOperationForSend('天气怎么样', [confirmBody]),
    ).toBeNull()
  })

  it('prefers the latest parseable target among several', () => {
    const older =
      'authorizeNewOperation target: prior_operation_id=op-old write_fingerprint=fp-old'
    const newer =
      'authorizeNewOperation target: prior_operation_id=op-new write_fingerprint=fp-new'
    expect(
      resolveAuthorizeNewOperationForSend('confirm re-run', [older, newer]),
    ).toEqual({ priorOperationId: 'op-new', writeFingerprint: 'fp-new' })
  })

  it('recognizes confirm phrases', () => {
    expect(isAuthorizeNewOperationPhrase('确认重跑')).toBe(true)
    expect(isAuthorizeNewOperationPhrase('confirm re-run')).toBe(true)
    expect(isAuthorizeNewOperationPhrase('hello')).toBe(false)
  })
})
