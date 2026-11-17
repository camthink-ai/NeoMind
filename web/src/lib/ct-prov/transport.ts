/**
 * Transport-agnostic CT-Prov session port.
 * BLE GATT (fragmented) and SoftAP HTTP (whole envelope) share this interface.
 */

import {
  decodeEnvelope,
  encodeRequest,
  encodeResponse,
  type JsonObject,
  type ProvRequest,
  type ProvResponse,
} from './envelope'
import { ProvOp, ProvErrorCode, resultOpName } from './ops'

export interface ProvTransport {
  /** Send a request and resolve with the matching response envelope. */
  sendRequest(request: ProvRequest, opts?: { timeoutMs?: number }): Promise<ProvResponse>
  /** Release resources; safe to call repeatedly. */
  close(): Promise<void>
}

export interface FakeTransportOptions {
  deviceName?: string
  serialNumber?: string
  model?: string
  firmware?: string
  power?: string[]
  /** Sections returned by communication.capabilities.get. */
  capabilities?: JsonObject
  /** Optional error code for config.set (e.g. FIELD_NOT_SUPPORTED). */
  rejectConfigSet?: string
  /** When false, config.get returns UNSUPPORTED_OPERATION. */
  supportsConfigGet?: boolean
  rejectDeviceInfo?: boolean
}

const DEFAULT_CAPABILITIES: JsonObject = {
  sections: [
    {
      name: 'wifi',
      kind: 'network',
      configurable: true,
      scannable: true,
      fields: ['ssid', 'password'],
    },
    {
      name: 'mqtt',
      kind: 'service',
      configurable: true,
      fields: [
        'enable',
        'host',
        'port',
        'client_id',
        'username',
        'password',
        'tls',
        'topic',
        'qos',
      ],
    },
  ],
  current: { network: null },
}

/**
 * In-memory ProvTransport that mimics a CT-Prov F2 device at the envelope layer.
 * Used for M0 session-path tests (info → capabilities → config.set → close).
 */
export class FakeTransport implements ProvTransport {
  readonly opts: Required<
    Pick<
      FakeTransportOptions,
      | 'deviceName'
      | 'serialNumber'
      | 'model'
      | 'firmware'
      | 'power'
      | 'supportsConfigGet'
      | 'rejectDeviceInfo'
    >
  > &
    FakeTransportOptions

  /** Last config.set data observed (for assertions). */
  lastConfigSet: JsonObject | null = null
  /** Ops received in order. */
  readonly opLog: string[] = []
  closed = false
  disconnectedAfterClose = false

  constructor(options: FakeTransportOptions = {}) {
    this.opts = {
      deviceName: options.deviceName ?? 'NE101-A1B2C3',
      serialNumber: options.serialNumber ?? 'NE101202610270001',
      model: options.model ?? 'NE101',
      firmware: options.firmware ?? '1.8.0',
      power: options.power ?? ['battery'],
      capabilities: options.capabilities ?? DEFAULT_CAPABILITIES,
      rejectConfigSet: options.rejectConfigSet,
      supportsConfigGet: options.supportsConfigGet ?? true,
      rejectDeviceInfo: options.rejectDeviceInfo ?? false,
    }
  }

  async sendRequest(
    request: ProvRequest,
    _opts?: { timeoutMs?: number },
  ): Promise<ProvResponse> {
    if (this.closed) {
      throw new Error('FakeTransport is closed')
    }
    this.opLog.push(request.opName)

    const json = this.buildResponseJson(request)
    const env = decodeEnvelope(new TextEncoder().encode(json))
    if (env.kind !== 'response') {
      throw new Error('FakeTransport produced non-response envelope')
    }
    if (request.opName === ProvOp.sessionClose) {
      this.disconnectedAfterClose = true
    }
    return env
  }

  async close(): Promise<void> {
    this.closed = true
  }

  private buildResponseJson(request: ProvRequest): string {
    const { id, opName, data } = request

    if (opName === ProvOp.deviceInfoGet) {
      if (this.opts.rejectDeviceInfo) {
        return encodeResponse({
          id,
          op: resultOpName(opName),
          ok: false,
          error: {
            codeName: ProvErrorCode.unsupportedOperation,
            message: 'operation is not supported',
          },
        })
      }
      return encodeResponse({
        id,
        op: resultOpName(opName),
        ok: true,
        data: {
          device_name: this.opts.deviceName,
          serial_number: this.opts.serialNumber,
          model: this.opts.model,
          firmware: this.opts.firmware,
          power: this.opts.power,
          protocol_version: 1,
        },
      })
    }

    if (opName === ProvOp.communicationCapabilitiesGet) {
      return encodeResponse({
        id,
        op: resultOpName(opName),
        ok: true,
        data: this.opts.capabilities,
      })
    }

    if (opName === ProvOp.configSet) {
      this.lastConfigSet = { ...data }
      if (this.opts.rejectConfigSet) {
        return encodeResponse({
          id,
          op: resultOpName(opName),
          ok: false,
          error: {
            codeName: this.opts.rejectConfigSet,
            message: 'config.set rejected by fake device',
          },
        })
      }
      return encodeResponse({
        id,
        op: resultOpName(opName),
        ok: true,
        data: {
          stored: Object.keys(data),
          apply: 'reconnect',
        },
      })
    }

    if (opName === ProvOp.configGet) {
      if (!this.opts.supportsConfigGet) {
        return encodeResponse({
          id,
          op: resultOpName(opName),
          ok: false,
          error: {
            codeName: ProvErrorCode.unsupportedOperation,
            message: 'operation is not supported',
          },
        })
      }
      return encodeResponse({
        id,
        op: resultOpName(opName),
        ok: true,
        data: this.lastConfigSet ?? {},
      })
    }

    if (opName === ProvOp.sessionClose) {
      return encodeResponse({
        id,
        op: resultOpName(opName),
        ok: true,
        data: {},
      })
    }

    if (opName === ProvOp.communicationScan) {
      // CT-Prov/1 golden §9 shape (network_id / display_name / signal).
      return encodeResponse({
        id,
        op: resultOpName(opName),
        ok: true,
        data: {
          section: data.section ?? 'wifi',
          items: [
            {
              network_id: 'Office-2.4G',
              display_name: 'Office-2.4G',
              signal: -48,
              channel: 6,
            },
            {
              network_id: 'Guest',
              display_name: 'Guest',
              signal: -72,
              channel: 1,
            },
          ],
          more: false,
          cursor: '',
        },
      })
    }

    return encodeResponse({
      id,
      op: resultOpName(opName),
      ok: false,
      error: {
        codeName: ProvErrorCode.unsupportedOperation,
        message: `unknown op ${opName}`,
      },
    })
  }
}

/** Helper to build a ProvRequest for FakeTransport / session clients. */
export function makeRequest(
  id: number,
  op: string,
  data: JsonObject = {},
): ProvRequest {
  const json = encodeRequest({ id, op, data })
  const env = decodeEnvelope(new TextEncoder().encode(json))
  if (env.kind !== 'request') throw new Error('expected request')
  return env
}
