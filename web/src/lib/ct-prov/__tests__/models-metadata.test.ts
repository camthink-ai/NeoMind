import { describe, expect, it } from 'vitest'

import {
  cellularSectionFromFields,
  findCtProvType,
  isFriendlyFirmwareName,
  listApProvisionModels,
  modelNameFromCode,
  modelToDeviceType,
  pruneIncompleteConfigSections,
  resolveProvisionDisplayName,
  softapHostForModel,
  webhookSectionFromFields,
  type CtProvDeviceTypeRef,
} from '../models'

const TYPES: CtProvDeviceTypeRef[] = [
  {
    device_type: 'ne101_camera',
    ct_prov: {
      model_code: 1,
      model_name: 'NE101',
      ap_ssid_families: ['NE101'],
      softap_default_host: '192.168.1.1',
      supported_discovery: ['ble', 'ap'],
    },
  },
  {
    device_type: 'ne301_camera',
    ct_prov: {
      model_code: 2,
      model_name: 'NE301',
      ap_ssid_families: ['NE301'],
      softap_default_host: '192.168.10.10',
      supported_discovery: ['ble', 'ap'],
    },
  },
  {
    device_type: 'ne302_camera',
    ct_prov: {
      model_name: 'NE302',
      ap_ssid_families: ['NE302'],
      softap_default_host: '192.168.10.10',
      supported_discovery: ['ap'],
    },
  },
  {
    device_type: 'ne503_camera',
    ct_prov: {
      model_code: 3,
      model_name: 'NE503',
      ap_ssid_families: ['NE503'],
      softap_default_host: '10.0.0.1',
      supported_discovery: ['ble', 'ap'],
    },
  },
  {
    device_type: 'ek410_Eink',
    ct_prov: {
      model_name: 'EK410',
      ap_ssid_families: ['EK410'],
      softap_default_host: '192.168.4.1',
      supported_discovery: ['softap'],
    },
  },
]

describe('ct-prov DeviceTypes metadata resolution', () => {
  it('maps model → device_type and SoftAP host from ct_prov', () => {
    expect(modelToDeviceType('NE101', TYPES)).toBe('ne101_camera')
    expect(modelToDeviceType('NE503', TYPES)).toBe('ne503_camera')
    expect(modelToDeviceType('NE503C', TYPES)).toBe('ne503_camera')
    expect(modelToDeviceType('NE302', TYPES)).toBe('ne302_camera')
    expect(modelToDeviceType('EK410', TYPES)).toBe('ek410_Eink')
    expect(softapHostForModel('NE101', TYPES)).toBe('192.168.1.1')
    expect(softapHostForModel('NE301', TYPES)).toBe('192.168.10.10')
    expect(softapHostForModel('NE302', TYPES)).toBe('192.168.10.10')
    expect(softapHostForModel('NE503', TYPES)).toBe('10.0.0.1')
    expect(softapHostForModel('EK410', TYPES)).toBe('192.168.4.1')
  })

  it('falls back to plan hosts when types are missing', () => {
    expect(softapHostForModel('NE301')).toBe('192.168.10.10')
    expect(softapHostForModel('NE302')).toBe('192.168.10.10')
    expect(softapHostForModel('EK410')).toBe('192.168.4.1')
    expect(modelToDeviceType('NE503')).toBe('ne503_camera')
    expect(modelToDeviceType('EK410')).toBe('ek410_Eink')
  })

  it('resolves model_code and AP model list including softap discovery', () => {
    expect(modelNameFromCode(3, TYPES)).toBe('NE503')
    expect(findCtProvType(TYPES, 'NE101')?.device_type).toBe('ne101_camera')
    const models = listApProvisionModels(TYPES)
    expect(models.map((m) => m.model).sort()).toEqual([
      'EK410',
      'NE101',
      'NE301',
      'NE302',
      'NE503',
    ])
  })

  it('fallback SoftAP list includes NE302 and EK410', () => {
    const models = listApProvisionModels([])
    expect(models.map((m) => m.model)).toEqual([
      'NE101',
      'NE301',
      'NE302',
      'NE503',
      'EK410',
    ])
  })

  it('keeps all seeded SoftAP families when DeviceTypes only has one SoftAP type', () => {
    const partial: CtProvDeviceTypeRef[] = [
      {
        device_type: 'ek410_Eink',
        ct_prov: {
          model_name: 'EK410',
          ap_ssid_families: ['EK410'],
          softap_default_host: '192.168.4.1',
          supported_discovery: ['softap'],
        },
      },
      // Present but BLE-only — must not shrink the SoftAP family list.
      {
        device_type: 'ne101_camera',
        ct_prov: {
          model_name: 'NE101',
          supported_discovery: ['ble'],
        },
      },
    ]
    const models = listApProvisionModels(partial)
    expect(models.map((m) => m.model)).toEqual([
      'NE101',
      'NE301',
      'NE302',
      'NE503',
      'EK410',
    ])
    expect(models.find((m) => m.model === 'EK410')?.host).toBe('192.168.4.1')
    expect(models.find((m) => m.model === 'EK410')?.deviceType).toBe('ek410_Eink')
  })
})

describe('cellular / webhook section builders', () => {
  it('gates cellular fields by capabilities declaration', () => {
    const full = cellularSectionFromFields(
      {
        apn: 'cmiot.iot',
        username: 'u',
        password: 'p',
        pin: '1234',
        auth: 'chap',
        roaming: true,
      },
      ['apn', 'username', 'password', 'pin', 'auth'],
    )
    expect(full).toEqual({
      apn: 'cmiot.iot',
      username: 'u',
      password: 'p',
      pin: '1234',
      auth: 'chap',
    })
    // roaming not declared → omitted
    expect(full).not.toHaveProperty('roaming')

    // Pre-provisioned SIM: no apn in fields, empty payload → null
    expect(cellularSectionFromFields({ apn: '' }, ['username'])).toBeNull()

    // Wi‑Fi-only: APN declared but blank — must NOT emit {auth:"none"}
    // (NE101: INVALID_ARGUMENT cellular.apn required field missing).
    expect(
      cellularSectionFromFields({ apn: '', auth: 'none' }, [
        'apn',
        'username',
        'password',
        'pin',
        'auth',
      ]),
    ).toBeNull()
    expect(cellularSectionFromFields({ auth: 'none' }, ['auth'])).toBeNull()
  })

  it('builds webhook enable=true and skips blank enable=false', () => {
    // Default off + empty url → omit (NE101 requires url if webhook present)
    expect(
      webhookSectionFromFields(
        { enable: false, url: '', secret: 's', header: 'X: 1' },
        ['enable', 'url'],
      ),
    ).toBeNull()

    // Explicit disable with url still writes
    expect(
      webhookSectionFromFields(
        { enable: false, url: 'https://example.com/h', secret: '', header: '' },
        ['enable', 'url'],
      ),
    ).toEqual({ enable: false, url: 'https://example.com/h' })

    expect(
      webhookSectionFromFields(
        {
          enable: true,
          url: 'https://example.com/h',
          secret: 's',
          header: 'X-API-Key: abc',
        },
        ['enable', 'url', 'secret', 'header'],
      ),
    ).toEqual({
      enable: true,
      url: 'https://example.com/h',
      secret: 's',
      header: 'X-API-Key: abc',
    })
  })

  it('pruneIncompleteConfigSections drops stub optional sections', () => {
    const pruned = pruneIncompleteConfigSections({
      mqtt: { enable: true, host: '192.168.1.1', port: 1883 },
      wifi: { ssid: 'Office', password: 'x' },
      cellular: { auth: 'none' },
      webhook: { enable: false },
      halow: { ssid: '', password: '' },
    })
    expect(pruned).toEqual({
      mqtt: { enable: true, host: '192.168.1.1', port: 1883 },
      wifi: { ssid: 'Office', password: 'x' },
    })
    expect(
      pruneIncompleteConfigSections({
        cellular: { apn: 'cmiot.iot', auth: 'none' },
        webhook: { enable: true, url: 'https://example.com/h' },
      }),
    ).toEqual({
      cellular: { apn: 'cmiot.iot', auth: 'none' },
      webhook: { enable: true, url: 'https://example.com/h' },
    })
  })

  it('resolveProvisionDisplayName prefers operator name over SN-like firmware', () => {
    expect(
      resolveProvisionDisplayName({
        userName: ' 门口摄像头 ',
        firmwareName: 'NE101-A1B2C3',
        serialNumber: 'NE101-A1B2C3',
        modelName: 'NE101',
      }),
    ).toBe('门口摄像头')
    expect(
      resolveProvisionDisplayName({
        userName: '',
        firmwareName: 'NE101-A1B2C3',
        serialNumber: 'NE101-A1B2C3',
        modelName: 'NE101',
      }),
    ).toBe('NE101')
    expect(
      resolveProvisionDisplayName({
        userName: '  ',
        firmwareName: 'Lobby Cam',
        serialNumber: 'NE101-A1B2C3',
        modelName: 'NE101',
      }),
    ).toBe('NE101')
    expect(isFriendlyFirmwareName('Lobby Cam', 'NE101-A1B2C3')).toBe(true)
    expect(isFriendlyFirmwareName('NE101-A1B2C3', 'NE101-A1B2C3')).toBe(false)
  })
})
