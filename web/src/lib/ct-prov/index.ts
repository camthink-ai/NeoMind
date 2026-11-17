/**
 * CamThink CT-Prov/1 client library (TypeScript).
 *
 * Shared by Tauri desktop and browser. M0 delivers protocol framing, envelopes,
 * discovery parse, FakeTransport, and the info→capabilities→config.set→close path.
 * Real BLE GATT / SoftAP HTTP transports land in later milestones.
 */

export * from './constants'
export * from './ops'
export * from './fragment'
export * from './envelope'
export * from './discovery'
export * from './transport'
export * from './session'
export * from './models'
export * from './ble-channel'
export * from './gatt-transport'
export * from './softap-transport'
export * from './active-ble-session'
export {
  FRAGMENT_VECTORS,
  ENVELOPE_VECTORS,
  envelopeWireBytes,
  ADVERTISING_VECTORS,
  NAME_VECTORS,
  META_VECTORS,
  NEOLINK_VECTORS_REF,
  exactSizedJson,
} from './vectors'
