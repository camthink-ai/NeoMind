import { describe, expect, it } from 'vitest'

import {
  findCtProvType,
  listApProvisionModels,
  modelNameFromCode,
  modelToDeviceType,
  softapHostForModel,
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
    device_type: 'ne503_camera',
    ct_prov: {
      model_code: 3,
      model_name: 'NE503',
      ap_ssid_families: ['NE503'],
      softap_default_host: '10.0.0.1',
      supported_discovery: ['ble', 'ap'],
    },
  },
]

describe('ct-prov DeviceTypes metadata resolution', () => {
  it('maps model → device_type and SoftAP host from ct_prov', () => {
    expect(modelToDeviceType('NE101', TYPES)).toBe('ne101_camera')
    expect(modelToDeviceType('NE503', TYPES)).toBe('ne503_camera')
    expect(modelToDeviceType('NE503C', TYPES)).toBe('ne503_camera')
    expect(softapHostForModel('NE101', TYPES)).toBe('192.168.1.1')
    expect(softapHostForModel('NE301', TYPES)).toBe('192.168.10.10')
    expect(softapHostForModel('NE503', TYPES)).toBe('10.0.0.1')
  })

  it('falls back to plan hosts when types are missing', () => {
    expect(softapHostForModel('NE301')).toBe('192.168.10.10')
    expect(modelToDeviceType('NE503')).toBe('ne503_camera')
  })

  it('resolves model_code and AP model list', () => {
    expect(modelNameFromCode(3, TYPES)).toBe('NE503')
    expect(findCtProvType(TYPES, 'NE101')?.device_type).toBe('ne101_camera')
    const models = listApProvisionModels(TYPES)
    expect(models.map((m) => m.model).sort()).toEqual(['NE101', 'NE301', 'NE503'])
  })
})
