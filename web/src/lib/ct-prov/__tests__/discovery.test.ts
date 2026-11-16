import { describe, expect, it } from 'vitest'

import {
  applyScanResponse,
  metaMatchesAdvertisedName,
  parseAdvertisingData,
  parseApSsid,
  parseMeta,
} from '../discovery'
import { ADVERTISING_VECTORS, META_VECTORS, NAME_VECTORS } from '../vectors'

describe('ct-prov advertising / META golden vectors', () => {
  it('parses advertising data vectors', () => {
    for (const v of ADVERTISING_VECTORS) {
      const dev = parseAdvertisingData(v.advData)
      expect(dev, v.id).not.toBeNull()
      if (!dev) continue
      expect(dev.formatVersion, v.id).toBe(v.expect.formatVersion)
      if (v.expect.formatVersionSupported === false) {
        expect(dev.formatVersionSupported).toBe(false)
        expect(dev.payloadFieldsReliable).toBe(false)
        continue
      }
      expect(dev.modelCode).toBe(v.expect.modelCode)
      expect(dev.macSuffix).toBe(v.expect.macSuffix)
      expect(dev.busy).toBe(v.expect.busy)
      expect(dev.provisionable).toBe(v.expect.provisionable)
      if (v.expect.modelCodeKnown === false) {
        expect(dev.modelCodeName).toBeUndefined()
      } else if (typeof v.expect.modelCodeName === 'string') {
        expect(dev.modelCodeName).toBe(v.expect.modelCodeName)
      }
    }
  })

  it('applies scan-response name vectors against NE101 baseline adv', () => {
    const baseline = ADVERTISING_VECTORS.find((v) => v.id === 'adv-baseline-ne101')!
    const base = parseAdvertisingData(baseline.advData)!
    for (const v of NAME_VECTORS) {
      const applied = applyScanResponse(base, v.scanResponse)
      if (v.expect.name === null) {
        expect(applied.advertisedName, v.id).toBeUndefined()
        expect(applied.displayName, v.id).toBe(v.expect.tempDisplayName)
        expect(applied.nameSource, v.id).toBe('derived')
        if (v.expect.invalidCharset) {
          expect(applied.nameInvalid, v.id).toBe(true)
        }
      } else {
        expect(applied.advertisedName, v.id).toBe(v.expect.name)
        expect(applied.nameSource, v.id).toBe('scanResponse')
      }
    }
  })

  it('parses META vectors', () => {
    const baseline = ADVERTISING_VECTORS.find((v) => v.id === 'adv-baseline-ne101')!
    const base = parseAdvertisingData(baseline.advData)!
    // Each META vector pairs with its own Scan Response name (§4.3).
    const rspById: Record<string, number[] | null | undefined> = {
      'meta-baseline': NAME_VECTORS.find((n) => n.id === 'name-baseline')!.scanResponse,
      'meta-max-20': NAME_VECTORS.find((n) => n.id === 'name-max-17')!.scanResponse,
      'meta-name-mismatch': NAME_VECTORS.find((n) => n.id === 'name-baseline')!.scanResponse,
    }
    for (const v of META_VECTORS) {
      const info = parseMeta(v.meta)
      expect(info, v.id).not.toBeNull()
      if (!info) continue
      expect(info.protocolVersion).toBe(v.expect.protocolVersion)
      expect(info.busy).toBe(v.expect.busy)
      expect(info.provisionable).toBe(v.expect.provisionable)
      expect(info.name).toBe(v.expect.name)
      const advertisedName = applyScanResponse(base, rspById[v.id]).advertisedName
      expect(metaMatchesAdvertisedName(info, advertisedName)).toBe(
        v.expect.matchesScanResponse === true,
      )
    }
  })

  it('parses SoftAP SSID forms used by AP discovery', () => {
    expect(parseApSsid('CT-AP-A1B2C3')).toEqual({
      macSuffix: 'A1B2C3',
      style: 'ct-ap',
    })
    expect(parseApSsid('NE503_A1B2C3')).toEqual({
      macSuffix: 'A1B2C3',
      modelHint: 'NE503',
      style: 'legacy-family',
    })
    expect(parseApSsid('Office-WiFi')).toBeNull()
  })
})
