/**
 * JSON envelope golden vectors — ported from neolink packages/test_vectors
 * @ a22c7f28deef27119a9adb03e07f8e7e2bd07a1f
 */

export interface EnvelopeVector {
  id: string
  description: string
  jsonText: string
  /** When set, decode must succeed and match this shape summary. */
  expect?: Record<string, unknown>
  /** When set, decode must fail with this stable reason. */
  invalidReason?: string
  /** Optional explicit wire bytes (BOM case). */
  wireBytes?: number[]
}

const utf8Encoder = new TextEncoder()

export function envelopeWireBytes(v: EnvelopeVector): Uint8Array {
  if (v.wireBytes) return new Uint8Array(v.wireBytes)
  return utf8Encoder.encode(v.jsonText)
}

export const ENVELOPE_VECTORS: EnvelopeVector[] = [
  // ---- requests (six ops) ----
  {
    id: 'env-req-device-info',
    description: '§7 request: device.info.get',
    jsonText: '{"id":1,"op":"device.info.get","data":{}}',
    expect: { kind: 'request', id: 1, op: 'device.info.get', data: {} },
  },
  {
    id: 'env-req-capabilities',
    description: '§8 request: communication.capabilities.get',
    jsonText: '{"id":12,"op":"communication.capabilities.get","data":{}}',
    expect: {
      kind: 'request',
      id: 12,
      op: 'communication.capabilities.get',
      data: {},
    },
  },
  {
    id: 'env-req-scan',
    description: '§9 request: communication.scan (no cursor)',
    jsonText: '{"id":3,"op":"communication.scan","data":{"section":"wifi"}}',
    expect: {
      kind: 'request',
      id: 3,
      op: 'communication.scan',
      data: { section: 'wifi' },
    },
  },
  {
    id: 'env-req-scan-cursor',
    description: '§9 request: scan with opaque cursor',
    jsonText:
      '{"id":4,"op":"communication.scan","data":{"section":"wifi","cursor":"opaque-1"}}',
    expect: {
      kind: 'request',
      id: 4,
      op: 'communication.scan',
      data: { section: 'wifi', cursor: 'opaque-1' },
    },
  },
  {
    id: 'env-req-config-set-multi',
    description: '§10 request: config.set multi-section (wifi + mqtt)',
    jsonText:
      '{"id":20,"op":"config.set","data":{"wifi":{"ssid":"Office-2.4G","password":"example-password"},' +
      '"mqtt":{"enable":true,"host":"mqtt.example.com","port":8883,"username":"device",' +
      '"password":"example-secret","tls":true}}}',
    expect: {
      kind: 'request',
      id: 20,
      op: 'config.set',
      data: {
        wifi: { ssid: 'Office-2.4G', password: 'example-password' },
        mqtt: {
          enable: true,
          host: 'mqtt.example.com',
          port: 8883,
          username: 'device',
          password: 'example-secret',
          tls: true,
        },
      },
    },
  },
  {
    id: 'env-req-config-get',
    description: '§10 optional request: config.get',
    jsonText: '{"id":21,"op":"config.get","data":{}}',
    expect: { kind: 'request', id: 21, op: 'config.get', data: {} },
  },
  {
    id: 'env-req-session-close',
    description: '§11 request: session.close',
    jsonText: '{"id":32,"op":"session.close","data":{}}',
    expect: { kind: 'request', id: 32, op: 'session.close', data: {} },
  },

  // ---- success responses ----
  {
    id: 'env-resp-device-info-ne101',
    description: '§7 response: NE101 battery form',
    jsonText:
      '{"id":1,"op":"device.info.result","ok":true,"data":{"device_name":"NE101-A1B2C3",' +
      '"serial_number":"NE101202610270001","model":"NE101","firmware":"1.8.0",' +
      '"power":["battery"],"protocol_version":1}}',
    expect: {
      kind: 'response',
      id: 1,
      op: 'device.info.result',
      ok: true,
      data: {
        device_name: 'NE101-A1B2C3',
        serial_number: 'NE101202610270001',
        model: 'NE101',
        firmware: '1.8.0',
        power: ['battery'],
        protocol_version: 1,
      },
    },
  },
  {
    id: 'env-resp-device-info-ne301',
    description: '§7 response: NE301 external power form',
    jsonText:
      '{"id":1,"op":"device.info.result","ok":true,"data":{"device_name":"NE301-84C102",' +
      '"serial_number":"NE301202610270001","model":"NE301","firmware":"1.8.0",' +
      '"power":["external"],"protocol_version":1}}',
    expect: {
      kind: 'response',
      id: 1,
      op: 'device.info.result',
      ok: true,
      data: {
        device_name: 'NE301-84C102',
        serial_number: 'NE301202610270001',
        model: 'NE301',
        firmware: '1.8.0',
        power: ['external'],
        protocol_version: 1,
      },
    },
  },
  {
    id: 'env-resp-config-set',
    description: '§10 response: multi-section write ok, apply=reconnect',
    jsonText:
      '{"id":20,"op":"config.set.result","ok":true,"data":{"stored":["wifi","mqtt"],"apply":"reconnect"}}',
    expect: {
      kind: 'response',
      id: 20,
      op: 'config.set.result',
      ok: true,
      data: { stored: ['wifi', 'mqtt'], apply: 'reconnect' },
    },
  },
  {
    id: 'env-resp-session-close',
    description: '§11 response: session.close always succeeds',
    jsonText: '{"id":32,"op":"session.close.result","ok":true,"data":{}}',
    expect: {
      kind: 'response',
      id: 32,
      op: 'session.close.result',
      ok: true,
      data: {},
    },
  },

  // ---- failure responses (representative + all nine codes covered) ----
  {
    id: 'env-err-unsupported-operation',
    description: 'UNSUPPORTED_OPERATION',
    jsonText:
      '{"id":21,"op":"config.get.result","ok":false,"error":{"code":"UNSUPPORTED_OPERATION",' +
      '"message":"operation is not supported"}}',
    expect: {
      kind: 'response',
      id: 21,
      op: 'config.get.result',
      ok: false,
      error: { code: 'UNSUPPORTED_OPERATION' },
    },
  },
  {
    id: 'env-err-busy',
    description: 'BUSY',
    jsonText:
      '{"id":3,"op":"communication.scan.result","ok":false,"error":{"code":"BUSY",' +
      '"message":"writing configuration"}}',
    expect: {
      kind: 'response',
      id: 3,
      op: 'communication.scan.result',
      ok: false,
      error: { code: 'BUSY' },
    },
  },
  {
    id: 'env-err-section-not-supported',
    description: 'SECTION_NOT_SUPPORTED',
    jsonText:
      '{"id":20,"op":"config.set.result","ok":false,"error":{"code":"SECTION_NOT_SUPPORTED",' +
      '"message":"section ethernet is not configurable"}}',
    expect: {
      kind: 'response',
      id: 20,
      op: 'config.set.result',
      ok: false,
      error: { code: 'SECTION_NOT_SUPPORTED' },
    },
  },
  {
    id: 'env-err-field-not-supported',
    description: 'FIELD_NOT_SUPPORTED',
    jsonText:
      '{"id":20,"op":"config.set.result","ok":false,"error":{"code":"FIELD_NOT_SUPPORTED",' +
      '"message":"field wifi.bssid is not declared"}}',
    expect: {
      kind: 'response',
      id: 20,
      op: 'config.set.result',
      ok: false,
      error: { code: 'FIELD_NOT_SUPPORTED' },
    },
  },
  {
    id: 'env-err-invalid-argument',
    description: 'INVALID_ARGUMENT',
    jsonText:
      '{"id":20,"op":"config.set.result","ok":false,"error":{"code":"INVALID_ARGUMENT",' +
      '"message":"mqtt.port out of range"}}',
    expect: {
      kind: 'response',
      id: 20,
      op: 'config.set.result',
      ok: false,
      error: { code: 'INVALID_ARGUMENT' },
    },
  },
  {
    id: 'env-err-storage-failed',
    description: 'STORAGE_FAILED',
    jsonText:
      '{"id":20,"op":"config.set.result","ok":false,"error":{"code":"STORAGE_FAILED",' +
      '"message":"nvs commit failed"}}',
    expect: {
      kind: 'response',
      id: 20,
      op: 'config.set.result',
      ok: false,
      error: { code: 'STORAGE_FAILED' },
    },
  },
  {
    id: 'env-err-internal-error',
    description: 'INTERNAL_ERROR',
    jsonText:
      '{"id":1,"op":"device.info.result","ok":false,"error":{"code":"INTERNAL_ERROR",' +
      '"message":"unexpected"}}',
    expect: {
      kind: 'response',
      id: 1,
      op: 'device.info.result',
      ok: false,
      error: { code: 'INTERNAL_ERROR' },
    },
  },
  {
    id: 'env-err-unsupported-version',
    description: 'UNSUPPORTED_VERSION',
    jsonText:
      '{"id":1,"op":"device.info.result","ok":false,"error":{"code":"UNSUPPORTED_VERSION",' +
      '"message":"protocol version not supported"}}',
    expect: {
      kind: 'response',
      id: 1,
      op: 'device.info.result',
      ok: false,
      error: { code: 'UNSUPPORTED_VERSION' },
    },
  },
  {
    id: 'env-err-invalid-message',
    description: 'INVALID_MESSAGE',
    jsonText:
      '{"id":1,"op":"device.info.result","ok":false,"error":{"code":"INVALID_MESSAGE",' +
      '"message":"malformed envelope"}}',
    expect: {
      kind: 'response',
      id: 1,
      op: 'device.info.result',
      ok: false,
      error: { code: 'INVALID_MESSAGE' },
    },
  },

  // ---- events + illegal inputs ----
  {
    id: 'env-event-protocol-error',
    description: '§6.4 async event: protocol.error',
    jsonText: '{"seq":8,"event":"protocol.error","data":{}}',
    expect: { kind: 'event', seq: 8, event: 'protocol.error', data: {} },
  },
  {
    id: 'env-unknown-op-request',
    description: 'Unknown op request remains parseable',
    jsonText: '{"id":99,"op":"device.reboot","data":{}}',
    expect: { kind: 'request', id: 99, op: 'device.reboot', data: {} },
  },
  {
    id: 'env-invalid-bom',
    description: 'Illegal: UTF-8 BOM prefix (§6 forbids BOM)',
    jsonText: '{"id":1,"op":"device.info.get","data":{}}',
    // EF BB BF + JSON — TextEncoder of a BOM char is unreliable across editors.
    wireBytes: [
      0xef, 0xbb, 0xbf, ...Array.from(utf8Encoder.encode('{"id":1,"op":"device.info.get","data":{}}')),
    ],
    invalidReason: 'bom',
  },
  {
    id: 'env-invalid-not-object',
    description: 'Illegal: top-level not object',
    jsonText: '[1,2,3]',
    invalidReason: 'not_object',
  },
  {
    id: 'env-invalid-float-int',
    description: 'Illegal: integer expressed as float',
    jsonText: '{"id":12.5,"op":"communication.capabilities.get","data":{}}',
    invalidReason: 'float_int',
  },
  {
    id: 'env-invalid-trailing-garbage',
    description: 'Illegal: trailing garbage after object',
    jsonText: '{"id":1,"op":"device.info.get","data":{}} extra',
    invalidReason: 'trailing_garbage',
  },
]
