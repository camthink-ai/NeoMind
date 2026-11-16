import { describe, expect, it } from 'vitest'

import {
  FRAGMENT_HEADER_SIZE,
  FRAGMENT_MAGIC_VERSION,
  MAX_LOGICAL_PACKET_BYTES,
  PACKET_KIND_EVENT,
  PACKET_KIND_PROTOCOL_ERROR,
  PACKET_KIND_REQUEST,
  PACKET_KIND_RESPONSE,
} from '../constants'
import {
  FragmentAssembler,
  packetJsonText,
  slicePacket,
  type ProvPacketKind,
} from '../fragment'
import { FRAGMENT_VECTORS } from '../vectors'

function kindOf(token: string): ProvPacketKind {
  switch (token) {
    case 'request':
      return 'request'
    case 'response':
      return 'response'
    case 'event':
      return 'event'
    case 'protocolError':
      return 'protocolError'
    default:
      throw new Error(`unknown packet_kind token: ${token}`)
  }
}

describe('ct-prov fragment constants', () => {
  it('matches pinned neolink golden-vector constants', () => {
    expect(FRAGMENT_MAGIC_VERSION).toBe(0xc1)
    expect(FRAGMENT_HEADER_SIZE).toBe(5)
    expect(MAX_LOGICAL_PACKET_BYTES).toBe(4096)
    expect(PACKET_KIND_REQUEST).toBe(0x01)
    expect(PACKET_KIND_RESPONSE).toBe(0x02)
    expect(PACKET_KIND_EVENT).toBe(0x03)
    expect(PACKET_KIND_PROTOCOL_ERROR).toBe(0x7f)
  })
})

describe('ct-prov fragment golden vectors', () => {
  it('reassembles legal vectors into the expected logical packet', () => {
    for (const v of FRAGMENT_VECTORS) {
      if (v.expect.type !== 'reassembled') continue
      const assembler = new FragmentAssembler(v.mtu)
      let last = null as ReturnType<FragmentAssembler['feed']> | null
      for (let i = 0; i < v.frames.length; i++) {
        last = assembler.feed(v.frames[i]!)
        if (i < v.frames.length - 1) {
          expect(last.type, `${v.id} frame ${i}`).toBe('assembling')
        }
      }
      expect(last?.type, `${v.id} final`).toBe('assembled')
      if (last?.type !== 'assembled') continue
      expect(last.packet.kind).toBe(kindOf(v.expect.packetKind))
      expect(last.packet.messageId).toBe(v.expect.messageId)
      expect(packetJsonText(last.packet)).toBe(v.expect.jsonText)
    }
  })

  it('rejects illegal slice / magic / order with stable reasons', () => {
    for (const v of FRAGMENT_VECTORS) {
      if (v.expect.type !== 'rejected') continue
      // incomplete is not rejected on feed — last fragment never arrives.
      if (v.expect.reason === 'incomplete') continue
      const assembler = new FragmentAssembler(v.mtu)
      let rejected = false
      for (const frame of v.frames) {
        const r = assembler.feed(frame)
        if (r.type === 'rejected') {
          expect(r.reason, v.id).toBe(v.expect.reason)
          rejected = true
          break
        }
      }
      expect(rejected, `${v.id} must reject`).toBe(true)
      expect(assembler.hasPartialAssembly, `${v.id} buffer cleared`).toBe(false)
    }
  })

  it('leaves incomplete assemblies hanging (frag-missing-last)', () => {
    const v = FRAGMENT_VECTORS.find((e) => e.id === 'frag-missing-last')!
    const assembler = new FragmentAssembler(v.mtu)
    for (const frame of v.frames) {
      expect(assembler.feed(frame).type).toBe('assembling')
    }
    expect(assembler.hasPartialAssembly).toBe(true)
  })

  it('slicePacket round-trips through the assembler byte-identically', () => {
    const jsonText = '{"id":1,"op":"device.info.get","data":{}}'
    for (const mtu of [23, 61, 247]) {
      const frames = slicePacket({
        mtu,
        kind: 'request',
        messageId: 7,
        jsonText,
      })
      const assembler = new FragmentAssembler(mtu)
      let last = null as ReturnType<FragmentAssembler['feed']> | null
      for (const frame of frames) {
        last = assembler.feed(frame)
      }
      expect(last?.type).toBe('assembled')
      if (last?.type === 'assembled') {
        expect(packetJsonText(last.packet)).toBe(jsonText)
      }
    }
  })

  it('assembles peer-MTU-256 frames even when local assembler thinks MTU=23', () => {
    // Regression from neolink: Windows MTU query fell back to 23 while peer
    // sliced a ~608B capabilities response at MTU 256 (248/248/112).
    const jsonText =
      '{"id":2,"op":"communication.capabilities.get.result","ok":true,' +
      '"data":{"sections":[{"name":"wifi","kind":"network","configurable":true,' +
      '"scannable":true,"fields":["ssid","password"]},{"name":"cellular","kind":"network",' +
      '"configurable":true,"scannable":false,"fields":["apn","username","password","pin","auth"]},' +
      '{"name":"mqtt","kind":"service","configurable":true,"scannable":false,' +
      '"fields":["enable","host","port"]},{"name":"webhook","kind":"service",' +
      '"configurable":true,"scannable":false,"fields":["enable","url","header"]}],' +
      '"current":{"network":null}}}'
    const frames = slicePacket({
      mtu: 256,
      kind: 'response',
      messageId: 5,
      jsonText,
    })
    expect(frames.length).toBeGreaterThan(1)
    const assembler = new FragmentAssembler(23)
    let last = null as ReturnType<FragmentAssembler['feed']> | null
    for (const frame of frames) {
      last = assembler.feed(frame)
    }
    expect(last?.type).toBe('assembled')
    if (last?.type === 'assembled') {
      expect(packetJsonText(last.packet)).toBe(jsonText)
    }
  })
})

