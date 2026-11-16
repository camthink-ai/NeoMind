/**
 * Advertising / Scan Response / META parsers (§4).
 * Ported from neolink packages/ble_adapter discovery.dart @ NEOLINK_REF_COMMIT.
 */

import { CT_PROV_SERVICE_UUID_LE } from './constants'

/** Known model_code → name (unknown codes still parse and may connect). */
export const MODEL_CODE_NAMES: Readonly<Record<number, string>> = {
  0x01: 'NE101',
  0x02: 'NE301',
  // SoftAP already recognizes NE503; BLE code reserved for firmware alignment.
  0x03: 'NE503',
}

const NAME_PATTERN = /^[A-Z0-9-]{9,17}$/

export type NameSource = 'scanResponse' | 'derived'

export interface CtDiscoveredDevice {
  formatVersion: number
  formatVersionSupported: boolean
  /** False when format_version is unsupported — payload fields must not be trusted. */
  payloadFieldsReliable: boolean
  modelCode?: number
  modelCodeName?: string
  macSuffix?: string
  busy?: boolean
  provisionable?: boolean
  advertisedName?: string
  displayName?: string
  nameSource: NameSource
  nameInvalid: boolean
}

export interface CtMetaInfo {
  protocolVersion: number
  busy: boolean
  provisionable: boolean
  name: string
}

function tempName(modelName: string | undefined, macSuffix: string): string {
  return `${modelName ?? 'NE'}-${macSuffix}`
}

function macSuffixHex(bytes: Uint8Array): string {
  return Array.from(bytes)
    .map((b) => b.toString(16).padStart(2, '0').toUpperCase())
    .join('')
}

/**
 * Parse Advertising Data. Returns null when length/AD structure/UUID mismatch
 * (caller should filter by Service UUID first).
 */
export function parseAdvertisingData(
  advData: Uint8Array | number[],
): CtDiscoveredDevice | null {
  const bytes = advData instanceof Uint8Array ? advData : new Uint8Array(advData)
  if (bytes.length !== 27) return null
  if (bytes[0] !== 0x02 || bytes[1] !== 0x01 || bytes[2] !== 0x06) return null
  if (bytes[3] !== 0x17 || bytes[4] !== 0x21) return null
  for (let i = 0; i < CT_PROV_SERVICE_UUID_LE.length; i++) {
    if (bytes[5 + i] !== CT_PROV_SERVICE_UUID_LE[i]) return null
  }

  const formatVersion = bytes[21]!
  const supported = formatVersion === 0x01
  if (!supported) {
    return {
      formatVersion,
      formatVersionSupported: false,
      payloadFieldsReliable: false,
      nameSource: 'derived',
      nameInvalid: false,
    }
  }

  const modelCode = bytes[22]!
  const macSuffix = macSuffixHex(bytes.subarray(23, 26))
  const flags = bytes[26]!
  const busy = (flags & 0x01) !== 0
  // BUSY=1 with PROVISIONABLE=1 is a server violation — treat as not provisionable.
  const provisionable = !busy && (flags & 0x02) !== 0
  const modelCodeName = MODEL_CODE_NAMES[modelCode]

  return {
    formatVersion,
    formatVersionSupported: true,
    payloadFieldsReliable: true,
    modelCode,
    modelCodeName,
    macSuffix,
    busy,
    provisionable,
    displayName: tempName(modelCodeName, macSuffix),
    nameSource: 'derived',
    nameInvalid: false,
  }
}

/** Apply Complete Local Name from Scan Response. Returns a new record. */
export function applyScanResponse(
  device: CtDiscoveredDevice,
  scanResponse: Uint8Array | number[] | null | undefined,
): CtDiscoveredDevice {
  let name: string | undefined
  let invalid = false
  if (scanResponse != null) {
    const bytes =
      scanResponse instanceof Uint8Array ? scanResponse : new Uint8Array(scanResponse)
    if (bytes.length >= 2 && bytes[1] === 0x09) {
      const candidate = String.fromCharCode(...bytes.subarray(2))
      if (NAME_PATTERN.test(candidate)) {
        name = candidate
      } else {
        invalid = true
      }
    }
  }
  return {
    ...device,
    advertisedName: name,
    displayName: name ?? device.displayName,
    nameSource: name == null ? 'derived' : 'scanResponse',
    nameInvalid: invalid,
  }
}

/**
 * Parse META characteristic (§4.3, ≤20 bytes).
 * Returns null for empty / unsupported version / length mismatch.
 */
export function parseMeta(meta: Uint8Array | number[]): CtMetaInfo | null {
  const bytes = meta instanceof Uint8Array ? meta : new Uint8Array(meta)
  if (bytes.length < 3 || bytes.length > 20) return null
  if (bytes[0] !== 0x01) return null
  if (bytes[2] !== bytes.length - 3) return null
  const flags = bytes[1]!
  const busy = (flags & 0x01) !== 0
  return {
    protocolVersion: bytes[0]!,
    busy,
    provisionable: !busy && (flags & 0x02) !== 0,
    name: String.fromCharCode(...bytes.subarray(3)),
  }
}

export function metaMatchesAdvertisedName(
  meta: CtMetaInfo,
  advertisedName: string | undefined,
): boolean {
  return advertisedName === meta.name
}

/** SoftAP SSID patterns: CT-AP-<MAC6> or <MODEL>_<MAC6>. */
export function parseApSsid(ssid: string): {
  macSuffix: string
  modelHint?: string
  style: 'ct-ap' | 'legacy-family'
} | null {
  const ct = /^CT-AP-([0-9A-Fa-f]{6})$/.exec(ssid)
  if (ct) {
    return { macSuffix: ct[1]!.toUpperCase(), style: 'ct-ap' }
  }
  const legacy = /^(NE\d+[A-Z0-9]*)_([0-9A-Fa-f]{6})$/.exec(ssid)
  if (legacy) {
    return {
      modelHint: legacy[1],
      macSuffix: legacy[2]!.toUpperCase(),
      style: 'legacy-family',
    }
  }
  return null
}
