import { describe, expect, it } from 'vitest'

import { ProvOp, resultOpName } from '../ops'
import {
  CtSessionClient,
  ProvSessionError,
  formatProvUserMessage,
  formatSsidForDisplay,
  parseNetworkScan,
} from '../session'
import { FakeTransport } from '../transport'
import {
  NEOLINK_REF_COMMIT,
  CT_PROV_PRODUCT_NAME,
  CT_PROV_SERVICE_UUID,
  CT_PROV_C2D_UUID,
  CT_PROV_D2C_UUID,
  CT_PROV_META_UUID,
  normalizeBleUuid,
} from '../constants'
import {
  CT_PROV_WEB_BLE_NAME_PREFIXES,
  ctProvWebBluetoothRequestOptions,
} from '../ble-channel'
import { NEOLINK_VECTORS_REF } from '../vectors'

describe('ct-prov FakeTransport session path', () => {
  it('pins the neolink reference commit used for golden vectors', () => {
    expect(NEOLINK_REF_COMMIT).toBe(NEOLINK_VECTORS_REF.commit)
    expect(CT_PROV_PRODUCT_NAME).toBe('CamThink CT-Prov')
  })

  it('exposes lowercase CT-Prov GATT UUIDs for Web Bluetooth', () => {
    for (const uuid of [
      CT_PROV_SERVICE_UUID,
      CT_PROV_C2D_UUID,
      CT_PROV_D2C_UUID,
      CT_PROV_META_UUID,
    ]) {
      expect(uuid).toBe(uuid.toLowerCase())
      expect(normalizeBleUuid(uuid.toUpperCase())).toBe(uuid)
    }
  })

  it('Web Bluetooth requestDevice ORs services filter with name prefixes', () => {
    const opts = ctProvWebBluetoothRequestOptions()
    expect(opts.filters).toEqual(
      expect.arrayContaining([
        { services: [CT_PROV_SERVICE_UUID] },
        ...CT_PROV_WEB_BLE_NAME_PREFIXES.map((namePrefix) => ({ namePrefix })),
      ]),
    )
    expect(opts.optionalServices).toContain(CT_PROV_SERVICE_UUID)
    // Legacy web path used namePrefix NE — must stay for Service-Data-only ADV.
    expect(CT_PROV_WEB_BLE_NAME_PREFIXES).toContain('NE')
    expect(CT_PROV_WEB_BLE_NAME_PREFIXES).toContain('EK410')
  })

  it('runs device.info.get → capabilities.get → config.set → session.close', async () => {
    const transport = new FakeTransport({ model: 'NE101' })
    const client = new CtSessionClient(transport)

    const result = await client.runProvisioningPath({
      wifi: { ssid: 'Office-2.4G', password: 'example-password' },
      mqtt: {
        enable: true,
        host: 'mqtt.example.com',
        port: 8883,
        username: 'device',
        password: 'example-secret',
        tls: true,
      },
    })

    expect(result.deviceInfo.model).toBe('NE101')
    expect(result.deviceInfo.protocolVersion).toBe(1)
    expect(result.capabilities.sections.map((s) => s.name)).toContain('wifi')
    expect(result.configSet.stored).toEqual(['wifi', 'mqtt'])
    expect(result.configSet.apply).toBe('reconnect')

    expect(transport.opLog).toEqual([
      ProvOp.deviceInfoGet,
      ProvOp.communicationCapabilitiesGet,
      ProvOp.configSet,
      ProvOp.sessionClose,
    ])
    expect(transport.lastConfigSet).toMatchObject({
      wifi: { ssid: 'Office-2.4G' },
      mqtt: { host: 'mqtt.example.com', port: 8883 },
    })
    expect(transport.disconnectedAfterClose).toBe(true)

    await client.dispose()
    expect(transport.closed).toBe(true)
  })

  it('surfaces config.set rejection without closing the session', async () => {
    const transport = new FakeTransport({
      rejectConfigSet: 'FIELD_NOT_SUPPORTED',
    })
    const client = new CtSessionClient(transport)
    await client.open()
    await client.fetchDeviceInfo()
    await client.fetchCapabilities()
    await expect(
      client.setConfig({ wifi: { ssid: 'x', bssid: 'aa:bb' } }),
    ).rejects.toBeInstanceOf(ProvSessionError)
    expect(transport.opLog).not.toContain(ProvOp.sessionClose)
    await client.dispose()
  })

  it('scans wifi via communication.scan and sorts by rssi', async () => {
    const transport = new FakeTransport({ model: 'NE101' })
    const client = new CtSessionClient(transport)
    await client.open()
    const caps = await client.fetchCapabilities()
    const wifi = caps.sections.find((s) => s.name === 'wifi')
    expect(wifi?.scannable).toBe(true)
    const networks = await client.scanNetworks('wifi')
    expect(networks.map((n) => n.ssid)).toEqual(['Office-2.4G', 'Guest'])
    expect(networks[0]?.rssi).toBeGreaterThan(networks[1]?.rssi ?? -999)
    expect(transport.opLog).toContain(ProvOp.communicationScan)
    await client.dispose()
  })

  it('preserves leading/trailing SSID spaces from scan results', () => {
    const spaced = '  Office-2.4G  '
    const page = parseNetworkScan({
      kind: 'response',
      id: 1,
      ok: true,
      opName: resultOpName(ProvOp.communicationScan),
      data: {
        section: 'wifi',
        items: [{ ssid: spaced, rssi: -40 }, { ssid: '', rssi: -90 }],
        more: false,
      },
    })
    expect(page.items).toHaveLength(1)
    expect(page.items[0]?.ssid).toBe(spaced)
    // Submit value stays exact; display helper must not mutate it.
    expect(formatSsidForDisplay(page.items[0]!.ssid)).toBe(spaced)
  })

  it('parses CT-Prov/1 golden scan items (network_id / display_name / signal)', () => {
    const page = parseNetworkScan({
      kind: 'response',
      id: 3,
      ok: true,
      opName: resultOpName(ProvOp.communicationScan),
      data: {
        section: 'wifi',
        items: [
          {
            network_id: 'Office-2.4G',
            display_name: 'Office-2.4G',
            signal: -48,
            channel: 6,
          },
          {
            network_id: 'Lab-5G',
            display_name: 'Lab 5 GHz',
            signal: -71,
            channel: 36,
          },
          // Missing identity — skipped
          { display_name: 'orphan', signal: -90 },
        ],
        more: false,
        cursor: '',
      },
    })
    expect(page.items).toHaveLength(2)
    expect(page.items[0]).toMatchObject({
      ssid: 'Office-2.4G',
      displayName: 'Office-2.4G',
      rssi: -48,
      channel: 6,
    })
    expect(page.items[1]).toMatchObject({
      ssid: 'Lab-5G',
      displayName: 'Lab 5 GHz',
      rssi: -71,
    })
  })

  it('maps wifi_init INTERNAL_ERROR to a stable user code', () => {
    const err = new ProvSessionError(
      'internal_error',
      'INTERNAL_ERROR: wifi_init failed',
    )
    expect(formatProvUserMessage(err)).toBe('wifi_init_failed')
    expect(formatProvUserMessage('ProvSessionError(internal_error: INTERNAL_ERROR: wifi_init failed)')).toBe(
      'wifi_init_failed',
    )
  })

  it('maps STORAGE_FAILED wifi associate and GATT timeout to stable codes', () => {
    expect(
      formatProvUserMessage(
        new ProvSessionError('storage_failed', 'STORAGE_FAILED: wifi associate failed'),
      ),
    ).toBe('wifi_associate_failed')
    expect(formatProvUserMessage(new ProvSessionError('storage_failed', 'STORAGE_FAILED: nvs write'))).toBe(
      'storage_failed',
    )
    expect(formatProvUserMessage(new Error('CT-Prov request timed out after 15000ms'))).toBe(
      'request_timeout',
    )
  })

  it('maps any section.field required-missing to stable codes', () => {
    expect(
      formatProvUserMessage(
        new ProvSessionError('invalid_argument', 'INVALID_ARGUMENT: cellular.apn: required field missing'),
      ),
    ).toBe('cellular_apn_required')
    expect(
      formatProvUserMessage(
        new ProvSessionError('invalid_argument', 'INVALID_ARGUMENT: webhook.url: required field missing'),
      ),
    ).toBe('webhook_url_required')
    expect(
      formatProvUserMessage(
        new ProvSessionError('invalid_argument', 'INVALID_ARGUMENT: foo.bar: required field missing'),
      ),
    ).toBe('required_field_missing')
  })
})
