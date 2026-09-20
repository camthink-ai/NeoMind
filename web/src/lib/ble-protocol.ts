// BLE Provisioning Protocol — ESP-IDF wifi_prov_mgr (Security0)
//
// Uses ESP-IDF standard provisioning over BLE with protobuf encoding.
// Security0 = no encryption, so we can encode protobuf manually without
// pulling in protobufjs.
//
// Proto structure (from ESP-IDF wifi_config.proto):
//   WiFiConfigPayload {
//     WiFiConfigMsgType msg = 1;           // enum varint
//     oneof payload {
//       CmdGetStatus    cmd_get_status    = 10;
//       RespGetStatus   resp_get_status   = 11;
//       CmdSetConfig    cmd_set_config    = 12;
//       RespSetConfig   resp_set_config   = 13;
//       CmdApplyConfig  cmd_apply_config  = 14;
//       RespApplyConfig resp_apply_config = 15;
//     }
//   }
//   CmdSetConfig { bytes ssid = 1; bytes passphrase = 2; }
//   CmdGetStatus {} (empty)
//   RespSetConfig { Status status = 1; }
//   RespGetStatus { Status status = 1; WifiStationState sta_state = 2;
//                   oneof { WifiConnectFailedReason fail_reason = 10;
//                           WifiConnectedState connected = 11; } }

// ---------------------------------------------------------------------------
// Service & Characteristic UUIDs (ESP-IDF standard)
// ---------------------------------------------------------------------------

/** BLE provisioning service UUID (ESP-IDF standard, must match firmware) */
export const BLE_PROV_SERVICE_UUID = '0000fffe-0000-1000-8000-00805f9b34fb'

/** Scan endpoint */
export const BLE_CHAR_SCAN = '0000ff50-0000-1000-8000-00805f9b34fb'
/** Session endpoint (Security0 handshake) */
export const BLE_CHAR_SESSION = '0000ff51-0000-1000-8000-00805f9b34fb'
/** Config endpoint (WiFi credentials + status) */
export const BLE_CHAR_CONFIG = '0000ff52-0000-1000-8000-00805f9b34fb'
/** Custom config endpoint (UUID = 0xFF53 — raw NimBLE GATT characteristic) */
export const BLE_CHAR_MQTT = '0000ff53-0000-1000-8000-00805f9b34fb'

// ---------------------------------------------------------------------------
// Enum values (from ESP-IDF proto definitions)
// ---------------------------------------------------------------------------

export const enum WifiState {
  Connected = 0,
  Connecting = 1,
  Disconnected = 2,
  ConnectionFailed = 3,
}

export const enum ProvStatus {
  Success = 0,
}

/** WiFiConfigMsgType enum values */
const enum ConfigMsgType {
  TypeCmdGetStatus = 0,
  TypeRespGetStatus = 1,
  TypeCmdSetConfig = 2,
  TypeRespSetConfig = 3,
  TypeCmdApplyConfig = 4,
  TypeRespApplyConfig = 5,
}

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

export interface BleProvisionResult {
  success: boolean
  wifiState: WifiState
  error?: string
}

/** MQTT configuration returned by the pre-register API */
export interface BleMqttConfig {
  host: string
  port: number
  username: string
  password: string
  topic_prefix: string
  client_id: string
}

/** Device network mode reported via BLE */
export type BleNetmod = 'wifi' | 'halow' | 'cat1'

export const NETMOD_LABELS: Record<BleNetmod, string> = {
  wifi: 'WiFi',
  halow: 'WiFi HaLow',
  cat1: 'Cat.1 LTE',
}

/** Maps BLE device model strings to NeoMind device type identifiers (fallback) */
export const MODEL_TO_DEVICE_TYPE: Record<string, string> = {
  NE101: 'ne101_camera',
  NE301: 'ne301_camera',
}

/** Derive device type from model string: NE101 → ne101_camera */
export function modelToDeviceType(model: string): string | null {
  // Check known models first
  if (MODEL_TO_DEVICE_TYPE[model]) return MODEL_TO_DEVICE_TYPE[model]
  // Generic pattern: uppercase model → lowercase + suffix
  if (/^[A-Z]+\d+$/.test(model)) return `${model.toLowerCase()}_camera`
  return null
}

/** Parse BLE advertised device name into model and MAC suffix.
 *  e.g. "NE101_70B0E2" → { model: "NE101", macSuffix: "70B0E2" }
 *       "NE301-A1B2C3" → { model: "NE301", macSuffix: "A1B2C3" }
 */
export function parseBleDeviceName(name: string): { model: string; mac: string; macSuffix: string } | null {
  const match = name.match(/^([A-Z]+\d+)[-_]([0-9A-Fa-f]+)$/)
  if (!match) return null
  const model = match[1]
  const hex = match[2]
  // Full MAC (12 hex chars) or suffix (6 hex chars, old firmware)
  const macSuffix = hex.length === 12 ? hex.slice(6) : hex
  const mac = hex.length === 12
    ? `${hex.slice(0,2)}:${hex.slice(2,4)}:${hex.slice(4,6)}:${hex.slice(6,8)}:${hex.slice(8,10)}:${hex.slice(10,12)}`
    : macSuffix
  return { model, mac, macSuffix }
}

/** Encode MQTT config as JSON for the custom BLE MQTT endpoint */
export function encodeMqttConfig(config: {
  host: string
  port: number
  username: string
  password: string
  topic_prefix: string
  client_id: string
}): Uint8Array {
  return new TextEncoder().encode(JSON.stringify(config))
}

// ---------------------------------------------------------------------------
// Minimal protobuf encoder/decoder
// ---------------------------------------------------------------------------

function encodeVarint(value: number): Uint8Array {
  const bytes: number[] = []
  do {
    let b = value & 0x7f
    value >>>= 7
    if (value > 0) b |= 0x80
    bytes.push(b)
  } while (value > 0)
  return new Uint8Array(bytes)
}

function decodeVarint(data: Uint8Array, offset: number): [number, number] {
  let value = 0, shift = 0
  while (offset < data.length) {
    const b = data[offset++]
    value |= (b & 0x7f) << shift
    if (!(b & 0x80)) break
    shift += 7
  }
  return [value, offset]
}

/** Encode a protobuf bytes/string field (field_number, raw bytes) */
function encodeBytesField(field: number, payload: Uint8Array): Uint8Array {
  const tag = encodeVarint((field << 3) | 2) // wire type 2 = length-delimited
  const len = encodeVarint(payload.length)
  const out = new Uint8Array(tag.length + len.length + payload.length)
  let off = 0
  out.set(tag, off); off += tag.length
  out.set(len, off); off += len.length
  out.set(payload, off)
  return out
}

/** Encode a protobuf string field (field_number, UTF-8 value) */
function _encodeStringField(field: number, value: string): Uint8Array {
  return encodeBytesField(field, new TextEncoder().encode(value))
}

/** Encode a varint field (field_number, value) */
function encodeVarintField(field: number, value: number): Uint8Array {
  const tag = encodeVarint((field << 3) | 0) // wire type 0 = varint
  const val = encodeVarint(value)
  const out = new Uint8Array(tag.length + val.length)
  out.set(tag, 0)
  out.set(val, tag.length)
  return out
}

/** Encode a nested message field (field_number, serialized inner message) */
function encodeMessageField(field: number, inner: Uint8Array): Uint8Array {
  return encodeBytesField(field, inner)
}

/** Concatenate multiple Uint8Arrays */
function concat(...parts: Uint8Array[]): Uint8Array {
  const total = parts.reduce((s, p) => s + p.length, 0)
  const out = new Uint8Array(total)
  let off = 0
  for (const p of parts) { out.set(p, off); off += p.length }
  return out
}

// ---------------------------------------------------------------------------
// WiFiConfigPayload encoder
// ---------------------------------------------------------------------------

/**
 * Encode WiFiConfigPayload with CmdSetConfig.
 * WiFiConfigPayload { msg = TypeCmdSetConfig; cmd_set_config = { ssid, passphrase } }
 */
export function encodeSetConfig(ssid: string, passphrase: string): Uint8Array {
  // Inner CmdSetConfig { bytes ssid = 1; bytes passphrase = 2; }
  const inner = concat(
    encodeBytesField(1, new TextEncoder().encode(ssid)),
    encodeBytesField(2, new TextEncoder().encode(passphrase)),
  )
  // WiFiConfigPayload { msg = 2; cmd_set_config = inner }
  return concat(
    encodeVarintField(1, ConfigMsgType.TypeCmdSetConfig),  // msg = TypeCmdSetConfig
    encodeMessageField(12, inner),                          // cmd_set_config (field 12!)
  )
}

/**
 * Encode WiFiConfigPayload with CmdGetStatus.
 * WiFiConfigPayload { msg = TypeCmdGetStatus; cmd_get_status = {} }
 */
export function encodeGetStatus(): Uint8Array {
  // CmdGetStatus is empty, but we still need the WiFiConfigPayload wrapper
  return concat(
    encodeVarintField(1, ConfigMsgType.TypeCmdGetStatus),   // msg = TypeCmdGetStatus
    encodeMessageField(10, new Uint8Array(0)),              // cmd_get_status = {} (field 10!)
  )
}

// ---------------------------------------------------------------------------
// WiFiConfigPayload decoder
// ---------------------------------------------------------------------------

/**
 * Decode RespSetConfig from WiFiConfigPayload.
 * WiFiConfigPayload.resp_set_config { Status status = 1; }
 */
export function decodeRespSetConfig(data: Uint8Array): number {
  // Parse WiFiConfigPayload to find resp_set_config (field 13)
  const inner = extractField(data, 13)
  if (!inner || inner.length === 0) return -1

  // Parse RespSetConfig { Status status = 1; }
  const [tag, off1] = decodeVarint(inner, 0)
  if ((tag >> 3) === 1) {
    const [val] = decodeVarint(inner, off1)
    return val
  }
  return -1
}

/**
 * Decode RespGetStatus from WiFiConfigPayload.
 * WiFiConfigPayload.resp_get_status { Status status = 1; WifiStationState sta_state = 2; ... }
 */
export function decodeRespGetStatus(data: Uint8Array): {
  status: number
  wifiState: number
  failedReason: number
} {
  // Parse WiFiConfigPayload to find resp_get_status (field 11)
  const inner = extractField(data, 11)
  if (!inner || inner.length === 0) return { status: 0, wifiState: 0, failedReason: 0 }

  // Parse RespGetStatus
  let status = 0, wifiState = 0, failedReason = 0, offset = 0
  while (offset < inner.length) {
    const [tag, off1] = decodeVarint(inner, offset)
    offset = off1
    const fieldNum = tag >> 3
    const wireType = tag & 7
    if (wireType === 0) {
      const [val, off2] = decodeVarint(inner, offset)
      offset = off2
      if (fieldNum === 1) status = val
      else if (fieldNum === 2) wifiState = val
      else if (fieldNum === 10) failedReason = val
    } else if (wireType === 2) {
      const [len, off2] = decodeVarint(inner, offset)
      offset = off2 + len
    } else {
      break
    }
  }
  return { status, wifiState, failedReason }
}

/** Extract a length-delimited field from a protobuf message */
function extractField(data: Uint8Array, targetField: number): Uint8Array | null {
  let offset = 0
  while (offset < data.length) {
    const [tag, off1] = decodeVarint(data, offset)
    offset = off1
    const fieldNum = tag >> 3
    const wireType = tag & 7
    if (wireType === 0) {
      const [, off2] = decodeVarint(data, offset)
      offset = off2
    } else if (wireType === 2) {
      const [len, off2] = decodeVarint(data, offset)
      offset = off2
      if (fieldNum === targetField) {
        return data.slice(offset, offset + len)
      }
      offset += len
    } else {
      break
    }
  }
  return null
}

// ---------------------------------------------------------------------------
// BLE MTU-aware write
// ---------------------------------------------------------------------------

/** Default BLE ATT MTU payload size (23 byte MTU - 3 byte ATT header) */
const BLE_DEFAULT_MTU_PAYLOAD = 20

/**
 * Write a Uint8Array to a BLE characteristic, chunking if the payload
 * exceeds the negotiated MTU. Uses writeValueWithResponse for reliability.
 *
 * @param char  The BLE characteristic to write to
 * @param data  The full payload
 * @param mtu   Negotiated MTU payload size (default: 20)
 */
export async function writeWithMtu(
  char: BluetoothRemoteGATTCharacteristic,
  data: Uint8Array,
  mtu: number = BLE_DEFAULT_MTU_PAYLOAD,
): Promise<void> {
  for (let offset = 0; offset < data.length; offset += mtu) {
    const chunk = data.slice(offset, Math.min(offset + mtu, data.length))
    await char.writeValueWithResponse(chunk)
  }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

const decoder = new TextDecoder()

/** Decode DataView to string */
export function decodeText(value: DataView): string {
  return decoder.decode(value)
}

/** Convert DataView to Uint8Array for protobuf decoding */
export function viewToBytes(value: DataView): Uint8Array {
  return new Uint8Array(value.buffer, value.byteOffset, value.byteLength)
}
