/**
 * CT-Prov BLE GATT ProvTransport: envelope ↔ fragment pump over CtGattChannel.
 */

import { DEFAULT_REQUEST_TIMEOUT_MS } from './constants'
import type { CtGattChannel } from './ble-channel'
import {
  decodeEnvelope,
  encodeRequest,
  type ProvRequest,
  type ProvResponse,
} from './envelope'
import { FragmentAssembler, slicePacket, MessageIdAllocator } from './fragment'
import type { ProvTransport } from './transport'

export class BleGattCtTransport implements ProvTransport {
  private channel: CtGattChannel
  private assembler: FragmentAssembler
  private messageIds = new MessageIdAllocator()
  private pending = new Map<
    number,
    { resolve: (r: ProvResponse) => void; reject: (e: Error) => void }
  >()
  private started = false
  private closed = false

  constructor(channel: CtGattChannel) {
    this.channel = channel
    this.assembler = new FragmentAssembler(channel.mtu)
  }

  /** Start D2C subscription + fragment pump. Call once after connect. */
  async start(): Promise<void> {
    if (this.started) return
    this.started = true
    await this.channel.subscribeD2c((frame) => this.onFrame(frame))
  }

  private onFrame(frame: Uint8Array): void {
    const result = this.assembler.feed(frame)
    if (result.type === 'assembling') return
    if (result.type === 'rejected') return
    try {
      const env = decodeEnvelope(result.packet.bytes)
      if (env.kind !== 'response') return
      const pending = this.pending.get(env.id)
      if (!pending) return // §6.4 ignore unmatched
      this.pending.delete(env.id)
      pending.resolve(env)
    } catch {
      // Envelope decode failure — leave request to time out
    }
  }

  async sendRequest(
    request: ProvRequest,
    opts?: { timeoutMs?: number },
  ): Promise<ProvResponse> {
    if (this.closed) throw new Error('BleGattCtTransport is closed')
    if (!this.started) await this.start()
    if (this.pending.has(request.id)) {
      throw new Error(`request id ${request.id} already in flight`)
    }

    const timeoutMs = opts?.timeoutMs ?? DEFAULT_REQUEST_TIMEOUT_MS
    const responsePromise = new Promise<ProvResponse>((resolve, reject) => {
      this.pending.set(request.id, { resolve, reject })
    })

    const jsonText = encodeRequest({
      id: request.id,
      op: request.opName,
      data: request.data,
    })
    const frames = slicePacket({
      mtu: this.channel.mtu,
      kind: 'request',
      messageId: this.messageIds.next(),
      jsonText,
    })

    try {
      for (const frame of frames) {
        if (this.closed) break
        await this.channel.writeC2d(frame)
      }
      return await Promise.race([
        responsePromise,
        new Promise<ProvResponse>((_, reject) => {
          setTimeout(
            () => reject(new Error(`CT-Prov request timed out after ${timeoutMs}ms`)),
            timeoutMs,
          )
        }),
      ])
    } finally {
      this.pending.delete(request.id)
    }
  }

  async close(): Promise<void> {
    this.closed = true
    for (const [, p] of this.pending) {
      p.reject(new Error('transport closed before response arrived'))
    }
    this.pending.clear()
    this.channel.disconnect()
  }
}
