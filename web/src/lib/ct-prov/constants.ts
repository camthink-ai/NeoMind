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

export const CT_PROV_SERVICE_UUID = '7B1E0001-9D5B-4B5B-8F6D-2A4C0B7E0100'
export const CT_PROV_C2D_UUID = '7B1E0002-9D5B-4B5B-8F6D-2A4C0B7E0100'
export const CT_PROV_D2C_UUID = '7B1E0003-9D5B-4B5B-8F6D-2A4C0B7E0100'
export const CT_PROV_META_UUID = '7B1E0004-9D5B-4B5B-8F6D-2A4C0B7E0100'

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

/** Default SoftAP gateway hosts by model family. */
export const SOFTAP_DEFAULT_HOST: Readonly<Record<string, string>> = {
  NE101: '192.168.1.1',
  NE301: '192.168.10.10',
  NE503: '10.0.0.1',
}

export const DEFAULT_REQUEST_TIMEOUT_MS = 15_000
