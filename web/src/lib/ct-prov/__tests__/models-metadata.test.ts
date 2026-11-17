import { describe, expect, it } from 'vitest'

import {
  cellularSectionFromFields,
  findCtProvType,
  listApProvisionModels,
  modelNameFromCode,
  modelToDeviceType,
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
  })

  it('builds webhook enable=false and gates secret/header', () => {
    expect(
      webhookSectionFromFields(
        { enable: false, url: '', secret: 's', header: 'X: 1' },
        ['enable', 'url'],
      ),
    ).toEqual({ enable: false })

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
})
