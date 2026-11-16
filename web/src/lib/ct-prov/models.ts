/**
 * Model / SoftAP host mapping for CamThink CT-Prov UI.
 * Full DeviceTypes metadata lands in NM-158; these stubs match plan §2.6.
 */

import { SOFTAP_DEFAULT_HOST } from './constants'

/** Maps CT model strings to NeoMind device type identifiers. */
export const MODEL_TO_DEVICE_TYPE: Readonly<Record<string, string>> = {
  NE101: 'ne101_camera',
  NE301: 'ne301_camera',
  NE503: 'ne503_camera',
}

/** Derive device type from model string: NE101 → ne101_camera. */
export function modelToDeviceType(model: string): string | null {
  if (MODEL_TO_DEVICE_TYPE[model]) return MODEL_TO_DEVICE_TYPE[model]!
  if (/^[A-Z]+\d+[A-Z0-9]*$/.test(model)) return `${model.toLowerCase()}_camera`
  return null
}

/** SoftAP gateway host for a model family (NE503C → NE503). */
export function softapHostForModel(model: string): string {
  const upper = model.toUpperCase()
  for (const family of Object.keys(SOFTAP_DEFAULT_HOST)) {
    if (upper === family || upper.startsWith(family)) {
      return SOFTAP_DEFAULT_HOST[family]!
    }
  }
  return SOFTAP_DEFAULT_HOST.NE101!
}

export function softapEndpointForHost(host: string): string {
  return `http://${host}/api/v1/ctprov`
}

/** MQTT config returned by POST /devices/ble-provision (resolve phase). */
export interface CtMqttConfig {
  host: string
  port: number
  username: string
  password: string
  topic_prefix: string
  client_id: string
}

/** Build CT `mqtt` section from NeoMind resolve response. */
export function mqttSectionFromResolve(cfg: CtMqttConfig): Record<string, unknown> {
  return {
    enable: true,
    host: cfg.host,
    port: cfg.port,
    username: cfg.username || undefined,
    password: cfg.password || undefined,
    client_id: cfg.client_id,
    tls: cfg.port === 8883,
    topic: cfg.topic_prefix || undefined,
  }
}
