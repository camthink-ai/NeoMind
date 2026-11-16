/**
 * GATT fragment golden vectors — ported from neolink packages/test_vectors
 * @ a22c7f28deef27119a9adb03e07f8e7e2bd07a1f
 */

import {
  FRAGMENT_HEADER_SIZE,
  FRAGMENT_MAGIC_VERSION,
  PACKET_KIND_EVENT,
  PACKET_KIND_REQUEST,
  PACKET_KIND_RESPONSE,
} from '../constants'

export type FragmentExpect =
  | { type: 'reassembled'; packetKind: string; messageId: number; jsonText: string }
  | { type: 'rejected'; reason: string }

export interface FragmentVector {
  id: string
  description: string
  mtu: number
  frames: number[][]
  expect: FragmentExpect
}

const utf8Encoder = new TextEncoder()

function frames(opts: {
  mtu: number
  kind: number
  messageId: number
  jsonText: string
}): number[][] {
  const bytes = utf8Encoder.encode(opts.jsonText)
  const maxPayload = opts.mtu - 3 - FRAGMENT_HEADER_SIZE
  const out: number[][] = []
  for (let i = 0; i < bytes.length; i += maxPayload) {
    const end = Math.min(i + maxPayload, bytes.length)
    const chunk = Array.from(bytes.subarray(i, end))
    out.push([
      FRAGMENT_MAGIC_VERSION,
      opts.kind,
      opts.messageId,
      out.length,
      Math.ceil(bytes.length / maxPayload),
      ...chunk,
    ])
  }
  return out
}

/** Build a legal config.set JSON whose UTF-8 length is exactly `total`. */
export function exactSizedJson(total: number): string {
  const head = '{"id":20,"op":"config.set","data":{"wifi":{"ssid":"'
  const tail = '"}}}'
  const pad = total - head.length - tail.length
  if (pad < 1) throw new Error(`total too small: ${total}`)
  return head + 'A'.repeat(pad) + tail
}

const DEVICE_INFO_REQ = '{"id":1,"op":"device.info.get","data":{}}'
const PROTOCOL_ERROR_EVENT =
  '{"seq":2,"event":"protocol.error","data":{"code":"INVALID_MESSAGE","message":"fragment reassembly timeout"}}'

const DEVICE_INFO_RESP =
  '{"id":1,"op":"device.info.result","ok":true,"data":{"device_name":"NE101-A1B2C3",' +
  '"serial_number":"NE101202610270001","model":"NE101","firmware":"1.8.0",' +
  '"power":["battery"],"protocol_version":1}}'

function cloneFrames(src: number[][]): number[][] {
  return src.map((f) => [...f])
}

export const FRAGMENT_VECTORS: FragmentVector[] = [
  {
    id: 'frag-single-large-mtu',
    description: 'Single fragment: MTU 61 (payload_max 53) fits 41B request',
    mtu: 61,
    frames: frames({
      mtu: 61,
      kind: PACKET_KIND_REQUEST,
      messageId: 7,
      jsonText: DEVICE_INFO_REQ,
    }),
    expect: {
      type: 'reassembled',
      packetKind: 'request',
      messageId: 7,
      jsonText: DEVICE_INFO_REQ,
    },
  },
  {
    id: 'frag-multi-mtu23',
    description: 'MTU 23 baseline: 41B request → 15/15/11 three fragments',
    mtu: 23,
    frames: frames({
      mtu: 23,
      kind: PACKET_KIND_REQUEST,
      messageId: 7,
      jsonText: DEVICE_INFO_REQ,
    }),
    expect: {
      type: 'reassembled',
      packetKind: 'request',
      messageId: 7,
      jsonText: DEVICE_INFO_REQ,
    },
  },
  {
    id: 'frag-max-3825-mtu23',
    description: 'MTU 23 max legal message: 255 × 15B = 3825B',
    mtu: 23,
    frames: frames({
      mtu: 23,
      kind: PACKET_KIND_REQUEST,
      messageId: 9,
      jsonText: exactSizedJson(3825),
    }),
    expect: {
      type: 'reassembled',
      packetKind: 'request',
      messageId: 9,
      jsonText: exactSizedJson(3825),
    },
  },
  {
    id: 'frag-oversize-4302',
    description: 'Oversized: MTU 247 → 18 × 239B = 4302B > 4096; must reject',
    mtu: 247,
    frames: frames({
      mtu: 247,
      kind: PACKET_KIND_REQUEST,
      messageId: 3,
      jsonText: exactSizedJson(4302),
    }),
    expect: { type: 'rejected', reason: 'oversized' },
  },
  {
    id: 'frag-response-kind',
    description: 'RESPONSE multi-fragment reassembly (MTU 23)',
    mtu: 23,
    frames: frames({
      mtu: 23,
      kind: PACKET_KIND_RESPONSE,
      messageId: 4,
      jsonText: DEVICE_INFO_RESP,
    }),
    expect: {
      type: 'reassembled',
      packetKind: 'response',
      messageId: 4,
      jsonText: DEVICE_INFO_RESP,
    },
  },
  {
    id: 'frag-event-kind',
    description: 'EVENT single fragment: protocol.error (MTU 131)',
    mtu: 131,
    frames: frames({
      mtu: 131,
      kind: PACKET_KIND_EVENT,
      messageId: 2,
      jsonText: PROTOCOL_ERROR_EVENT,
    }),
    expect: {
      type: 'reassembled',
      packetKind: 'event',
      messageId: 2,
      jsonText: PROTOCOL_ERROR_EVENT,
    },
  },
  (() => {
    const f = frames({
      mtu: 23,
      kind: PACKET_KIND_REQUEST,
      messageId: 7,
      jsonText: DEVICE_INFO_REQ,
    })
    f[1] = [0xc0, ...f[1]!.slice(1)]
    return {
      id: 'frag-bad-magic',
      description: 'Second fragment magic_version=0xC0 → reject bad_magic',
      mtu: 23,
      frames: f,
      expect: { type: 'rejected' as const, reason: 'bad_magic' },
    }
  })(),
  (() => {
    const f = frames({
      mtu: 23,
      kind: PACKET_KIND_REQUEST,
      messageId: 7,
      jsonText: DEVICE_INFO_REQ,
    })
    f[0] = [FRAGMENT_MAGIC_VERSION, 0x7e, ...f[0]!.slice(2)]
    return {
      id: 'frag-unknown-packet-kind',
      description: 'First fragment packet_kind=0x7E → reject unknown_packet_kind',
      mtu: 23,
      frames: f,
      expect: { type: 'rejected' as const, reason: 'unknown_packet_kind' },
    }
  })(),
  (() => {
    const f = frames({
      mtu: 23,
      kind: PACKET_KIND_REQUEST,
      messageId: 7,
      jsonText: DEVICE_INFO_REQ,
    })
    return {
      id: 'frag-out-of-order',
      description: 'Fragment order reversed (index 1 before 0) → out_of_order',
      mtu: 23,
      frames: [f[1]!, f[0]!, f[2]!],
      expect: { type: 'rejected' as const, reason: 'out_of_order' },
    }
  })(),
  (() => {
    const f = frames({
      mtu: 23,
      kind: PACKET_KIND_REQUEST,
      messageId: 7,
      jsonText: DEVICE_INFO_REQ,
    })
    return {
      id: 'frag-missing-last',
      description: 'fragment_count=3 but last never arrives → incomplete',
      mtu: 23,
      frames: f.slice(0, 2),
      expect: { type: 'rejected' as const, reason: 'incomplete' },
    }
  })(),
  (() => {
    const f = frames({
      mtu: 23,
      kind: PACKET_KIND_REQUEST,
      messageId: 7,
      jsonText: DEVICE_INFO_REQ,
    })
    return {
      id: 'frag-duplicate-index',
      description: 'index 0 duplicated → duplicate_fragment',
      mtu: 23,
      frames: [f[0]!, f[0]!, f[1]!, f[2]!],
      expect: { type: 'rejected' as const, reason: 'duplicate_fragment' },
    }
  })(),
  (() => {
    const f = cloneFrames(
      frames({
        mtu: 23,
        kind: PACKET_KIND_REQUEST,
        messageId: 7,
        jsonText: DEVICE_INFO_REQ,
      }),
    )
    f[1] = [
      FRAGMENT_MAGIC_VERSION,
      PACKET_KIND_REQUEST,
      7,
      1,
      2,
      ...f[1]!.slice(FRAGMENT_HEADER_SIZE),
    ]
    return {
      id: 'frag-header-changed',
      description: 'Same message_id mid-stream fragment_count 3→2 → header_changed',
      mtu: 23,
      frames: f,
      expect: { type: 'rejected' as const, reason: 'header_changed' },
    }
  })(),
]
