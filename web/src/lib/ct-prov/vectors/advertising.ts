/**
 * Advertising / META golden vectors — ported from neolink packages/test_vectors
 * @ a22c7f28deef27119a9adb03e07f8e7e2bd07a1f
 */

import { CT_PROV_SERVICE_UUID_LE } from '../constants'

export interface AdvertisingVector {
  id: string
  description: string
  advData: number[]
  expect: Record<string, unknown>
}

export interface NameVector {
  id: string
  description: string
  scanResponse: number[] | null
  expect: Record<string, unknown>
}

export interface MetaVector {
  id: string
  description: string
  meta: number[]
  expect: Record<string, unknown>
}

function adv(opts: {
  formatVersion: number
  modelCode: number
  macSuffix: number[]
  flags: number
}): number[] {
  return [
    0x02,
    0x01,
    0x06,
    0x17,
    0x21,
    ...Array.from(CT_PROV_SERVICE_UUID_LE),
    opts.formatVersion,
    opts.modelCode,
    ...opts.macSuffix,
    opts.flags,
  ]
}

function scanRsp(name: string): number[] {
  return [1 + name.length, 0x09, ...Array.from(name, (c) => c.charCodeAt(0))]
}

function meta(opts: { flags: number; name: string }): number[] {
  return [0x01, opts.flags, opts.name.length, ...Array.from(opts.name, (c) => c.charCodeAt(0))]
}

export const ADVERTISING_VECTORS: AdvertisingVector[] = [
  {
    id: 'adv-baseline-ne101',
    description: '§4.2.2 baseline: NE101 / A1B2C3 / PROVISIONABLE',
    advData: adv({
      formatVersion: 0x01,
      modelCode: 0x01,
      macSuffix: [0xa1, 0xb2, 0xc3],
      flags: 0x02,
    }),
    expect: {
      formatVersion: 1,
      modelCode: 0x01,
      modelCodeName: 'NE101',
      macSuffix: 'A1B2C3',
      busy: false,
      provisionable: true,
    },
  },
  {
    id: 'adv-ne301',
    description: 'NE301 / 84C102 / PROVISIONABLE',
    advData: adv({
      formatVersion: 0x01,
      modelCode: 0x02,
      macSuffix: [0x84, 0xc1, 0x02],
      flags: 0x02,
    }),
    expect: {
      formatVersion: 1,
      modelCode: 0x02,
      modelCodeName: 'NE301',
      macSuffix: '84C102',
      busy: false,
      provisionable: true,
    },
  },
  {
    id: 'adv-unknown-model',
    description: 'Unknown model_code=0x3C still parses',
    advData: adv({
      formatVersion: 0x01,
      modelCode: 0x3c,
      macSuffix: [0xa1, 0xb2, 0xc3],
      flags: 0x02,
    }),
    expect: {
      formatVersion: 1,
      modelCode: 0x3c,
      modelCodeKnown: false,
      macSuffix: 'A1B2C3',
      busy: false,
      provisionable: true,
    },
  },
  {
    id: 'adv-busy',
    description: 'BUSY=1 → not provisionable',
    advData: adv({
      formatVersion: 0x01,
      modelCode: 0x01,
      macSuffix: [0xa1, 0xb2, 0xc3],
      flags: 0x01,
    }),
    expect: {
      formatVersion: 1,
      modelCode: 0x01,
      modelCodeName: 'NE101',
      macSuffix: 'A1B2C3',
      busy: true,
      provisionable: false,
    },
  },
  {
    id: 'adv-busy-combo-invalid',
    description: 'BUSY+PROVISIONABLE both set → treat as not provisionable',
    advData: adv({
      formatVersion: 0x01,
      modelCode: 0x01,
      macSuffix: [0xa1, 0xb2, 0xc3],
      flags: 0x03,
    }),
    expect: {
      formatVersion: 1,
      modelCode: 0x01,
      modelCodeName: 'NE101',
      macSuffix: 'A1B2C3',
      busy: true,
      provisionable: false,
      comboViolation: true,
    },
  },
  {
    id: 'adv-reserved-bits',
    description: 'Reserved bits set (flags=0xFE) → ignore reserved',
    advData: adv({
      formatVersion: 0x01,
      modelCode: 0x01,
      macSuffix: [0xa1, 0xb2, 0xc3],
      flags: 0xfe,
    }),
    expect: {
      formatVersion: 1,
      modelCode: 0x01,
      modelCodeName: 'NE101',
      macSuffix: 'A1B2C3',
      busy: false,
      provisionable: true,
      reservedBitsIgnored: true,
    },
  },
  {
    id: 'adv-not-provisionable',
    description: 'flags=0 → not busy, not provisionable',
    advData: adv({
      formatVersion: 0x01,
      modelCode: 0x01,
      macSuffix: [0xa1, 0xb2, 0xc3],
      flags: 0x00,
    }),
    expect: {
      formatVersion: 1,
      modelCode: 0x01,
      modelCodeName: 'NE101',
      macSuffix: 'A1B2C3',
      busy: false,
      provisionable: false,
    },
  },
  {
    id: 'adv-unsupported-format',
    description: 'format_version=0x02 → stop interpreting payload fields',
    advData: adv({
      formatVersion: 0x02,
      modelCode: 0x01,
      macSuffix: [0xa1, 0xb2, 0xc3],
      flags: 0x02,
    }),
    expect: {
      formatVersion: 2,
      formatVersionSupported: false,
      payloadFieldsReliable: false,
    },
  },
]

export const NAME_VECTORS: NameVector[] = [
  {
    id: 'name-baseline',
    description: '§4.2.3 baseline: NE101-A1B2C3',
    scanResponse: scanRsp('NE101-A1B2C3'),
    expect: { name: 'NE101-A1B2C3', modelPrefix: 'NE101', macSuffix: 'A1B2C3' },
  },
  {
    id: 'name-max-17',
    description: 'Longest name 17B: MODEL 10 + - + MAC6',
    scanResponse: scanRsp('NE12345678-ABCDEF'),
    expect: {
      name: 'NE12345678-ABCDEF',
      modelPrefix: 'NE12345678',
      macSuffix: 'ABCDEF',
    },
  },
  {
    id: 'name-missing-scanrsp',
    description: 'Missing Scan Response → derived temp name',
    scanResponse: null,
    expect: {
      name: null,
      tempDisplayName: 'NE101-A1B2C3',
      source: 'derived',
    },
  },
  {
    id: 'name-invalid-charset',
    description: 'Lowercase name invalid → derived temp name',
    scanResponse: scanRsp('ne101-abc123'),
    expect: {
      name: null,
      tempDisplayName: 'NE101-A1B2C3',
      source: 'derived',
      invalidCharset: true,
    },
  },
]

export const META_VECTORS: MetaVector[] = [
  {
    id: 'meta-baseline',
    description: '§4.3 baseline META',
    meta: meta({ flags: 0x02, name: 'NE101-A1B2C3' }),
    expect: {
      protocolVersion: 1,
      busy: false,
      provisionable: true,
      name: 'NE101-A1B2C3',
      matchesScanResponse: true,
    },
  },
  {
    id: 'meta-max-20',
    description: '20B META upper bound',
    meta: meta({ flags: 0x02, name: 'NE12345678-ABCDEF' }),
    expect: {
      protocolVersion: 1,
      busy: false,
      provisionable: true,
      name: 'NE12345678-ABCDEF',
      matchesScanResponse: true,
    },
  },
  {
    id: 'meta-name-mismatch',
    description: 'META name differs from Scan Response',
    meta: meta({ flags: 0x02, name: 'NE301-84C102' }),
    expect: {
      protocolVersion: 1,
      busy: false,
      provisionable: true,
      name: 'NE301-84C102',
      matchesScanResponse: false,
    },
  },
]
