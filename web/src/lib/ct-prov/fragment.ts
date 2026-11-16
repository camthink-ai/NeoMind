/**
 * GATT fragment layer (§5): one-direction assembler + slicer.
 * Rejection reason strings are stable cross-impl identifiers matching golden vectors.
 */

import {
  FRAGMENT_HEADER_SIZE,
  FRAGMENT_MAGIC_VERSION,
  MAX_LOGICAL_PACKET_BYTES,
  PACKET_KIND_EVENT,
  PACKET_KIND_PROTOCOL_ERROR,
  PACKET_KIND_REQUEST,
  PACKET_KIND_RESPONSE,
} from './constants'

export type ProvPacketKind = 'request' | 'response' | 'event' | 'protocolError'

const KIND_TO_WIRE: Record<ProvPacketKind, number> = {
  request: PACKET_KIND_REQUEST,
  response: PACKET_KIND_RESPONSE,
  event: PACKET_KIND_EVENT,
  protocolError: PACKET_KIND_PROTOCOL_ERROR,
}

const WIRE_TO_KIND: Record<number, ProvPacketKind> = {
  [PACKET_KIND_REQUEST]: 'request',
  [PACKET_KIND_RESPONSE]: 'response',
  [PACKET_KIND_EVENT]: 'event',
  [PACKET_KIND_PROTOCOL_ERROR]: 'protocolError',
}

export function packetKindFromWire(byte: number): ProvPacketKind | null {
  return WIRE_TO_KIND[byte] ?? null
}

export function packetKindToWire(kind: ProvPacketKind): number {
  return KIND_TO_WIRE[kind]
}

export interface ProvPacket {
  kind: ProvPacketKind
  /** Fragment-layer message_id (0..255); distinct from envelope request id. */
  messageId: number
  bytes: Uint8Array
}

export type FeedResult =
  | { type: 'assembling' }
  | { type: 'assembled'; packet: ProvPacket }
  | { type: 'rejected'; reason: string }

const utf8Decoder = new TextDecoder('utf-8', { fatal: true })
const utf8Encoder = new TextEncoder()

export function packetJsonText(packet: ProvPacket): string {
  return utf8Decoder.decode(packet.bytes)
}

/** One-direction fragment assembler. Feed one ATT frame at a time. */
export class FragmentAssembler {
  readonly mtu: number
  readonly payloadMax: number

  private kindByte: number | null = null
  private messageId: number | null = null
  private count: number | null = null
  private expectedIndex = 0
  private buffer: number[] = []

  constructor(mtu: number) {
    this.mtu = mtu
    this.payloadMax = mtu - 3 - FRAGMENT_HEADER_SIZE
    if (this.payloadMax < 1) {
      throw new Error(`mtu too small: ${mtu} (payload_max must be at least 1)`)
    }
  }

  get hasPartialAssembly(): boolean {
    return this.kindByte !== null
  }

  feed(frame: Uint8Array | number[]): FeedResult {
    const bytes = frame instanceof Uint8Array ? frame : new Uint8Array(frame)
    if (bytes.length < FRAGMENT_HEADER_SIZE) return this.reject('malformed_frame')

    const magic = bytes[0]!
    const kindByte = bytes[1]!
    const messageId = bytes[2]!
    const index = bytes[3]!
    const count = bytes[4]!

    if (magic !== FRAGMENT_MAGIC_VERSION) return this.reject('bad_magic')
    if (packetKindFromWire(kindByte) === null) return this.reject('unknown_packet_kind')

    if (this.kindByte === null) {
      if (count < 1) return this.reject('malformed_frame')
      if (index !== 0) return this.reject('out_of_order')
      this.kindByte = kindByte
      this.messageId = messageId
      this.count = count
      this.expectedIndex = 0
    } else if (
      kindByte !== this.kindByte ||
      messageId !== this.messageId ||
      count !== this.count
    ) {
      return this.reject('header_changed')
    } else if (index < this.expectedIndex) {
      return this.reject('duplicate_fragment')
    } else if (index > this.expectedIndex) {
      return this.reject('out_of_order')
    }

    for (let i = FRAGMENT_HEADER_SIZE; i < bytes.length; i++) {
      this.buffer.push(bytes[i]!)
    }
    if (this.buffer.length > MAX_LOGICAL_PACKET_BYTES) return this.reject('oversized')

    const isFinal = index === this.count! - 1
    if (!isFinal) {
      // Non-final size is not hard-checked: local MTU may disagree with peer.
      this.expectedIndex = index + 1
      return { type: 'assembling' }
    }

    const packet: ProvPacket = {
      kind: packetKindFromWire(this.kindByte!)!,
      messageId: this.messageId!,
      bytes: new Uint8Array(this.buffer),
    }
    this.reset()
    return { type: 'assembled', packet }
  }

  private reject(reason: string): FeedResult {
    this.reset()
    return { type: 'rejected', reason }
  }

  private reset(): void {
    this.kindByte = null
    this.messageId = null
    this.count = null
    this.expectedIndex = 0
    this.buffer = []
  }
}

/**
 * Slice a logical packet into ATT frames (5B header + payload).
 * Non-final fragments are exactly payload_max bytes (vector-identical).
 */
export function slicePacket(opts: {
  mtu: number
  kind: ProvPacketKind
  messageId: number
  jsonText: string
}): Uint8Array[] {
  const payloadMax = opts.mtu - 3 - FRAGMENT_HEADER_SIZE
  if (payloadMax < 1) {
    throw new Error(`mtu too small: ${opts.mtu} (payload_max must be at least 1)`)
  }
  const payload = utf8Encoder.encode(opts.jsonText)
  if (payload.length === 0) {
    throw new Error('packet payload must not be empty')
  }
  if (payload.length > MAX_LOGICAL_PACKET_BYTES) {
    throw new Error(
      `packet exceeds limit: ${payload.length} > ${MAX_LOGICAL_PACKET_BYTES}`,
    )
  }
  const count = Math.ceil(payload.length / payloadMax)
  if (count > 255) {
    throw new Error(
      `needs ${count} fragments at MTU ${opts.mtu}, over fragment_count limit 255`,
    )
  }

  const frames: Uint8Array[] = []
  const kindByte = packetKindToWire(opts.kind)
  const messageId = opts.messageId % 256
  for (let i = 0; i < payload.length; i += payloadMax) {
    const end = Math.min(i + payloadMax, payload.length)
    const chunk = payload.subarray(i, end)
    const frame = new Uint8Array(FRAGMENT_HEADER_SIZE + chunk.length)
    frame[0] = FRAGMENT_MAGIC_VERSION
    frame[1] = kindByte
    frame[2] = messageId
    frame[3] = frames.length
    frame[4] = count
    frame.set(chunk, FRAGMENT_HEADER_SIZE)
    frames.push(frame)
  }
  return frames
}

/** Fragment-layer message_id allocator (wraps at 256). */
export class MessageIdAllocator {
  private nextId: number

  constructor(start = 0) {
    this.nextId = start % 256
  }

  next(): number {
    const id = this.nextId
    this.nextId = (this.nextId + 1) % 256
    return id
  }
}
