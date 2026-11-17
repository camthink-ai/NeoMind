/**
 * CamThink CT-Prov/1 constants.
 *
 * Protocol truth is pinned to Oreofilling/neolink @ NEOLINK_REF_COMMIT.
 * Golden vectors under ./vectors are ported from packages/test_vectors.
 */

/** Pinned neolink reference commit (shallow clone used for M0). */
export const NEOLINK_REF_COMMIT = 'a22c7f28deef27119a9adb03e07f8e7e2bd07a1f'

/** Human-facing protocol product name (not "NeoLink"). */
export const CT_PROV_PRODUCT_NAME = 'CamThink CT-Prov'

/** Protocol family label on the wire / docs. */
export const CT_PROV_PROTOCOL = 'CT-Prov/1'

/**
 * Web Bluetooth requires lowercase hex UUIDs in requestDevice / getPrimaryService /
 * getCharacteristic. Keep these canonical lowercase; Tauri blec accepts either case.
 */
export const CT_PROV_SERVICE_UUID = '7b1e0001-9d5b-4b5b-8f6d-2a4c0b7e0100'
export const CT_PROV_C2D_UUID = '7b1e0002-9d5b-4b5b-8f6d-2a4c0b7e0100'
export const CT_PROV_D2C_UUID = '7b1e0003-9d5b-4b5b-8f6d-2a4c0b7e0100'
export const CT_PROV_META_UUID = '7b1e0004-9d5b-4b5b-8f6d-2a4c0b7e0100'

/** Normalize a BLE UUID for Web Bluetooth (lowercase, strip braces). */
export function normalizeBleUuid(uuid: string): string {
  return uuid.toLowerCase().replace(/[{}]/g, '')
}

/** Service UUID little-endian bytes as they appear in Advertising Data. */
export const CT_PROV_SERVICE_UUID_LE = new Uint8Array([
  0x00, 0x01, 0x7e, 0x0b, 0x4c, 0x2a, 0x6d, 0x8f, 0x5b, 0x4b, 0x5b, 0x9d, 0x01,
  0x00, 0x1e, 0x7b,
])

export const FRAGMENT_MAGIC_VERSION = 0xc1
export const PACKET_KIND_REQUEST = 0x01
export const PACKET_KIND_RESPONSE = 0x02
export const PACKET_KIND_EVENT = 0x03
export const PACKET_KIND_PROTOCOL_ERROR = 0x7f
export const FRAGMENT_HEADER_SIZE = 5
export const MAX_LOGICAL_PACKET_BYTES = 4096

/** SoftAP HTTP path (whole envelope, no fragmentation). */
export const CT_PROV_SOFTAP_PATH = '/api/v1/ctprov'

/**
 * SoftAP Add Device tab visibility.
 * NM-222 SoftAP Wi‑Fi scan is assessment-only / deferred; keep SoftAP code
 * but hide the tab until that iteration lands. BLE (+ manual/auto) remain.
 */
export const CT_PROV_SOFTAP_UI_ENABLED = false

/** Default SoftAP gateway hosts by model family. */
export const SOFTAP_DEFAULT_HOST: Readonly<Record<string, string>> = {
  NE101: '192.168.1.1',
  NE301: '192.168.10.10',
  NE302: '192.168.10.10',
  NE503: '10.0.0.1',
  EK410: '192.168.4.1',
}

export const DEFAULT_REQUEST_TIMEOUT_MS = 15_000

/**
 * config.set often waits on device Wi‑Fi associate before answering.
 * EK410/BLE logs showed STORAGE_FAILED replies arriving ~20s after the write —
 * longer than the default 15s request timeout (UI falsely said "timed out").
 */
export const CONFIG_SET_TIMEOUT_MS = 45_000
