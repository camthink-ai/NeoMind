import {
  collectActivePendingConfirms,
  isAuthorizeNewOperationPhrase,
  parseAuthorizeNewOperationConfirm,
  parseAuthorizeNewOperationTarget,
  resolveAuthorizeNewOperationForSend,
  toAuthorizeRequest,
} from '../authorizeNewOperation'

const NOW = 1_700_000_000_000

function confirmLine(opts: {
  requestId: string
  opId: string
  fp: string
  validUntilMs: number
}): string {
  return (
    `Write blocked… authorizeNewOperation confirm: request_id=${opts.requestId} ` +
    `prior_operation_id=${opts.opId} write_fingerprint=${opts.fp} ` +
    `valid_until_ms=${opts.validUntilMs}`
  )
}

describe('parseAuthorizeNewOperationConfirm', () => {
  it('parses request_id + op + fingerprint + validity', () => {
    const text = confirmLine({
      requestId: 'req-abc',
      opId: 'op-abc',
      fp: 'run_now|{"agent_id":"A"}',
      validUntilMs: NOW + 60_000,
    })
    expect(parseAuthorizeNewOperationConfirm(text)).toEqual({
      requestId: 'req-abc',
      priorOperationId: 'op-abc',
      writeFingerprint: 'run_now|{"agent_id":"A"}',
      validUntilMs: NOW + 60_000,
    })
    expect(parseAuthorizeNewOperationTarget(text)).toEqual({
      priorOperationId: 'op-abc',
      writeFingerprint: 'run_now|{"agent_id":"A"}',
    })
  })

  it('rejects legacy target markers without request_id / validity', () => {
    const legacy =
      'authorizeNewOperation target: prior_operation_id=op-a write_fingerprint=fp-a'
    expect(parseAuthorizeNewOperationConfirm(legacy)).toBeNull()
    expect(parseAuthorizeNewOperationTarget(legacy)).toBeNull()
  })

  it('rejects plain text that only mentions markers', () => {
    expect(
      parseAuthorizeNewOperationConfirm(
        'please set prior_operation_id=op-x write_fingerprint=fp-x somehow',
      ),
    ).toBeNull()
    expect(
      parseAuthorizeNewOperationConfirm(
        'request_id=req-x prior_operation_id=op-x write_fingerprint=fp-x valid_until_ms=999',
      ),
    ).toBeNull()
  })
})

describe('resolveAuthorizeNewOperationForSend', () => {
  const activeA = {
    requestId: 'req-a',
    priorOperationId: 'op-a',
    writeFingerprint: 'fp-a',
    validUntilMs: NOW + 60_000,
  }
  const activeB = {
    requestId: 'req-b',
    priorOperationId: 'op-b',
    writeFingerprint: 'fp-b',
    validUntilMs: NOW + 60_000,
  }
  const expired = {
    requestId: 'req-old',
    priorOperationId: 'op-old',
    writeFingerprint: 'fp-old',
    validUntilMs: NOW - 1,
  }

  it('attaches the unique active pending confirm on a confirm phrase', () => {
    expect(
      resolveAuthorizeNewOperationForSend('确认重跑', [activeA], NOW),
    ).toEqual(toAuthorizeRequest(activeA))
  })

  it('does not attach on ordinary messages even with pending confirms', () => {
    expect(
      resolveAuthorizeNewOperationForSend('天气怎么样', [activeA], NOW),
    ).toBeNull()
  })

  it('multi pending confirms → phrase does not pick latest-B', () => {
    expect(
      resolveAuthorizeNewOperationForSend('confirm re-run', [activeA, activeB], NOW),
    ).toBeNull()
  })

  it('expired confirm is not usable by phrase shortcut', () => {
    expect(
      resolveAuthorizeNewOperationForSend('confirm re-run', [expired], NOW),
    ).toBeNull()
  })

  it('plain text with markers does not authorize', () => {
    expect(
      resolveAuthorizeNewOperationForSend(
        'authorizeNewOperation confirm: request_id=req-x prior_operation_id=op-x write_fingerprint=fp-x valid_until_ms=999',
        [activeA],
        NOW,
      ),
    ).toBeNull()
  })

  it('recognizes confirm phrases', () => {
    expect(isAuthorizeNewOperationPhrase('确认重跑')).toBe(true)
    expect(isAuthorizeNewOperationPhrase('confirm re-run')).toBe(true)
    expect(isAuthorizeNewOperationPhrase('hello')).toBe(false)
  })
})

describe('collectActivePendingConfirms', () => {
  it('keeps multiple active confirms and drops expired', () => {
    const texts = [
      confirmLine({
        requestId: 'req-a',
        opId: 'op-a',
        fp: 'fp-a',
        validUntilMs: NOW + 10_000,
      }),
      confirmLine({
        requestId: 'req-b',
        opId: 'op-b',
        fp: 'fp-b',
        validUntilMs: NOW + 10_000,
      }),
      confirmLine({
        requestId: 'req-old',
        opId: 'op-old',
        fp: 'fp-old',
        validUntilMs: NOW - 5,
      }),
      'unrelated prior_operation_id=noise write_fingerprint=noise',
    ]
    const pending = collectActivePendingConfirms(texts, NOW)
    expect(pending.map((c) => c.requestId).sort()).toEqual(['req-a', 'req-b'])
  })
})
