import { describe, expect, it } from 'vitest'

import { BleGattCtTransport } from '../gatt-transport'
import type { CtGattChannel } from '../ble-channel'
import { FragmentAssembler, slicePacket } from '../fragment'
import { encodeResponse } from '../envelope'
import { makeRequest } from '../transport'
import { ProvOp, resultOpName } from '../ops'
import { CtSessionClient } from '../session'

class FakeGattChannel implements CtGattChannel {
  mtu = 23
  private d2c: ((frame: Uint8Array) => void) | null = null
  private assembler = new FragmentAssembler(23)
  written: Uint8Array[] = []
  disconnected = false

  async writeC2d(frame: Uint8Array): Promise<void> {
    this.written.push(frame)
    const result = this.assembler.feed(frame)
    if (result.type !== 'assembled') return
    // Echo a device.info / capabilities / config.set / close response.
    const text = new TextDecoder().decode(result.packet.bytes)
    const req = JSON.parse(text) as { id: number; op: string; data: Record<string, unknown> }
    let data: Record<string, unknown> = {}
    if (req.op === ProvOp.deviceInfoGet) {
      data = {
        device_name: 'NE101-A1B2C3',
        serial_number: 'SN1',
        model: 'NE101',
        firmware: '1.8.0',
        power: ['battery'],
        protocol_version: 1,
      }
    } else if (req.op === ProvOp.communicationCapabilitiesGet) {
      data = {
        sections: [
          {
            name: 'wifi',
            kind: 'network',
            configurable: true,
            scannable: true,
            fields: ['ssid', 'password'],
          },
          {
            name: 'mqtt',
            kind: 'service',
            configurable: true,
            fields: ['enable', 'host', 'port'],
          },
        ],
        current: { network: null },
      }
    } else if (req.op === ProvOp.configSet) {
      data = { stored: Object.keys(req.data), apply: 'reconnect' }
    }
    const json = encodeResponse({
      id: req.id,
      op: resultOpName(req.op),
      ok: true,
      data,
    })
    const frames = slicePacket({
      mtu: this.mtu,
      kind: 'response',
      messageId: 1,
      jsonText: json,
    })
    // Deliver asynchronously like a real indicate stream.
    queueMicrotask(() => {
      for (const f of frames) this.d2c?.(f)
    })
  }

  async subscribeD2c(handler: (frame: Uint8Array) => void): Promise<void> {
    this.d2c = handler
  }

  async readMeta() {
    return null
  }

  disconnect(): void {
    this.disconnected = true
  }
}

describe('BleGattCtTransport + session over fake GATT', () => {
  it('runs info → capabilities → config.set → close over fragmented GATT', async () => {
    const channel = new FakeGattChannel()
    const transport = new BleGattCtTransport(channel)
    await transport.start()
    const client = new CtSessionClient(transport)
    const result = await client.runProvisioningPath({
      wifi: { ssid: 'Office', password: 'secret' },
      mqtt: { enable: true, host: 'mqtt.example.com', port: 1883 },
    })
    expect(result.deviceInfo.model).toBe('NE101')
    expect(result.capabilities.sections.map((s) => s.name)).toContain('wifi')
    expect(result.configSet.stored).toEqual(['wifi', 'mqtt'])
    expect(channel.written.length).toBeGreaterThan(1) // MTU 23 → multi-frame
    await client.dispose()
    expect(channel.disconnected).toBe(true)
  })

  it('rejects unmatched single-frame request timeout path is covered by FakeTransport elsewhere', async () => {
    const channel = new FakeGattChannel()
    const transport = new BleGattCtTransport(channel)
    await transport.start()
    const resp = await transport.sendRequest(makeRequest(42, ProvOp.sessionClose), {
      timeoutMs: 2000,
    })
    expect(resp.ok).toBe(true)
    expect(resp.id).toBe(42)
  })
})
