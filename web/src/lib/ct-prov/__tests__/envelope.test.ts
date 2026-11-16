import { describe, expect, it } from 'vitest'

import {
  decodeEnvelope,
  encodeRequest,
  ProvEnvelopeError,
  type ProvEnvelope,
} from '../envelope'
import { ENVELOPE_VECTORS, envelopeWireBytes } from '../vectors'

function summarize(env: ProvEnvelope): Record<string, unknown> {
  if (env.kind === 'request') {
    return { kind: 'request', id: env.id, op: env.opName, data: env.data }
  }
  if (env.kind === 'event') {
    return { kind: 'event', seq: env.seq, event: env.event, data: env.data }
  }
  if (env.ok) {
    return { kind: 'response', id: env.id, op: env.opName, ok: true, data: env.data ?? {} }
  }
  return {
    kind: 'response',
    id: env.id,
    op: env.opName,
    ok: false,
    error: { code: env.error?.codeName },
  }
}

describe('ct-prov envelope golden vectors', () => {
  it('decodes legal envelopes to the expected shape', () => {
    for (const v of ENVELOPE_VECTORS) {
      if (v.invalidReason) continue
      const env = decodeEnvelope(envelopeWireBytes(v))
      const summary = summarize(env)
      expect(summary, v.id).toEqual(v.expect)
    }
  })

  it('rejects illegal BOM / non-object / float-int / trailing garbage', () => {
    for (const v of ENVELOPE_VECTORS) {
      if (!v.invalidReason) continue
      try {
        decodeEnvelope(envelopeWireBytes(v))
        expect.fail(`${v.id} should have thrown`)
      } catch (err) {
        expect(err).toBeInstanceOf(ProvEnvelopeError)
        expect((err as ProvEnvelopeError).reason, v.id).toBe(v.invalidReason)
      }
    }
  })

  it('encodeRequest produces a decodable compact request', () => {
    const json = encodeRequest({
      id: 1,
      op: 'device.info.get',
      data: {},
    })
    const env = decodeEnvelope(new TextEncoder().encode(json))
    expect(env.kind).toBe('request')
    if (env.kind === 'request') {
      expect(env.id).toBe(1)
      expect(env.opName).toBe('device.info.get')
    }
  })
})
