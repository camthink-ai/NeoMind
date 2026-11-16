import { describe, expect, it, vi } from 'vitest'

import { SoftapHttpCtTransport, SoftapTransportError } from '../softap-transport'
import { encodeResponse } from '../envelope'
import { makeRequest } from '../transport'
import { ProvOp, resultOpName } from '../ops'
import { softapHostForModel, modelToDeviceType, mqttSectionFromResolve } from '../models'

describe('SoftapHttpCtTransport', () => {
  it('POSTs a whole envelope and returns the matching response', async () => {
    const body = encodeResponse({
      id: 1,
      op: resultOpName(ProvOp.deviceInfoGet),
      ok: true,
      data: {
        device_name: 'NE101-A1B2C3',
        serial_number: 'SN1',
        model: 'NE101',
        firmware: '1.8.0',
        power: ['battery'],
        protocol_version: 1,
      },
    })
    const fetchImpl = vi.fn(async () => new Response(body, { status: 200 }))
    const transport = new SoftapHttpCtTransport({
      model: 'NE101',
      fetchImpl: fetchImpl as unknown as typeof fetch,
    })
    expect(transport.endpointUrl).toBe('http://192.168.1.1/api/v1/ctprov')

    const resp = await transport.sendRequest(makeRequest(1, ProvOp.deviceInfoGet))
    expect(resp.ok).toBe(true)
    expect(resp.id).toBe(1)
    expect(fetchImpl).toHaveBeenCalledOnce()
    const call = fetchImpl.mock.calls[0] as unknown as [string, RequestInit]
    expect(call[1]?.method).toBe('POST')
    expect(String(call[1]?.body)).toContain('device.info.get')
  })

  it('maps unreachable SoftAP to ap_unreachable', async () => {
    const fetchImpl = vi.fn(async () => {
      throw new TypeError('Failed to fetch')
    })
    const transport = new SoftapHttpCtTransport({
      host: '10.0.0.1',
      fetchImpl: fetchImpl as unknown as typeof fetch,
    })
    await expect(
      transport.sendRequest(makeRequest(1, ProvOp.sessionClose)),
    ).rejects.toMatchObject({ code: 'ap_unreachable' } satisfies Partial<SoftapTransportError>)
  })
})

describe('ct-prov model stubs', () => {
  it('maps SoftAP hosts and device types for NE101/301/503', () => {
    expect(softapHostForModel('NE101')).toBe('192.168.1.1')
    expect(softapHostForModel('NE301')).toBe('192.168.10.10')
    expect(softapHostForModel('NE503')).toBe('10.0.0.1')
    expect(softapHostForModel('NE503C')).toBe('10.0.0.1')
    expect(modelToDeviceType('NE101')).toBe('ne101_camera')
    expect(modelToDeviceType('NE503')).toBe('ne503_camera')
  })

  it('builds mqtt section from resolve response', () => {
    expect(
      mqttSectionFromResolve({
        host: 'mqtt.example.com',
        port: 8883,
        username: 'u',
        password: 'p',
        topic_prefix: 'neomind/',
        client_id: 'dev-1',
      }),
    ).toMatchObject({
      enable: true,
      host: 'mqtt.example.com',
      port: 8883,
      tls: true,
      client_id: 'dev-1',
    })
  })
})
