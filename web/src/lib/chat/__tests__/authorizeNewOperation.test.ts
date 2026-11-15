import {
  confirmDisplayLabel,
  filterActivePendingConfirms,
  isAuthorizeNewOperationPhrase,
  removePendingConfirm,
  resolveAuthorizeNewOperationForSend,
  toAuthorizeRequest,
  upsertPendingConfirm,
} from '../authorizeNewOperation'

const NOW = 1_700_000_000_000

const spacedFp =
  'shell|neomind device control --device "Front Door Cam" --action reboot'

describe('toAuthorizeRequest', () => {
  it('passes spaced fingerprints via JSON fields (no text parse)', () => {
    const confirm = {
      requestId: 'req-space',
      priorOperationId: 'op-space',
      writeFingerprint: spacedFp,
      toolName: 'shell',
      target: 'Front Door Cam',
      validUntilMs: NOW + 60_000,
    }
    expect(toAuthorizeRequest(confirm)).toEqual({
      requestId: 'req-space',
      priorOperationId: 'op-space',
      writeFingerprint: spacedFp,
    })
    expect(confirmDisplayLabel(confirm)).toEqual({
      opId: 'op-space',
      target: 'Front Door Cam',
    })
  })
})

describe('pending confirm event lifecycle', () => {
  const activeA = {
    requestId: 'req-a',
    priorOperationId: 'op-a',
    writeFingerprint: 'fp-a',
    validUntilMs: NOW + 60_000,
  }
  const activeB = {
    requestId: 'req-b',
    priorOperationId: 'op-b',
    writeFingerprint: spacedFp,
    toolName: 'shell',
    target: 'Front Door Cam',
    validUntilMs: NOW + 60_000,
  }
  const expired = {
    requestId: 'req-old',
    priorOperationId: 'op-old',
    writeFingerprint: 'fp-old',
    validUntilMs: NOW - 1,
  }

  it('upserts from events and drops expired', () => {
    let pending = upsertPendingConfirm([], activeA, NOW)
    pending = upsertPendingConfirm(pending, activeB, NOW)
    pending = upsertPendingConfirm(pending, expired, NOW)
    expect(filterActivePendingConfirms(pending, NOW).map((c) => c.requestId).sort()).toEqual([
      'req-a',
      'req-b',
    ])
  })

  it('duplicate request same op reuses requestId (upsert)', () => {
    let pending = upsertPendingConfirm([], activeA, NOW)
    pending = upsertPendingConfirm(pending, { ...activeA, target: 'sensor-1' }, NOW)
    expect(pending).toHaveLength(1)
    expect(pending[0].requestId).toBe('req-a')
    expect(pending[0].target).toBe('sensor-1')
  })

  it('consumed / removed clears the pending chip', () => {
    const pending = removePendingConfirm([activeA, activeB], 'req-a')
    expect(pending.map((c) => c.requestId)).toEqual(['req-b'])
  })

  it('expired click is not attachable via phrase', () => {
    expect(
      resolveAuthorizeNewOperationForSend('confirm re-run', [expired], NOW),
    ).toBeNull()
  })

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

  it('recognizes confirm phrases', () => {
    expect(isAuthorizeNewOperationPhrase('确认重跑')).toBe(true)
    expect(isAuthorizeNewOperationPhrase('confirm re-run')).toBe(true)
    expect(isAuthorizeNewOperationPhrase('hello')).toBe(false)
  })
})
