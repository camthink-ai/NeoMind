/**
 * Model / SoftAP host mapping for CamThink CT-Prov UI.
 *
 * Prefer DeviceType.ct_prov metadata from NeoMind / DeviceTypes.
 * Plan §2.6 hosts are used only as safe fallbacks when types are missing.
 */

import { SOFTAP_DEFAULT_HOST } from './constants'

/** Plan §2.6 SoftAP hosts — last-resort fallbacks when DeviceTypes lack ct_prov. */
export const PLAN_SOFTAP_HOST_FALLBACK: Readonly<Record<string, string>> = {
  ...SOFTAP_DEFAULT_HOST,
}

/** Legacy hardcoded map kept as fallback when DeviceTypes are unloaded. */
export const MODEL_TO_DEVICE_TYPE_FALLBACK: Readonly<Record<string, string>> = {
  NE101: 'ne101_camera',
  NE301: 'ne301_camera',
  NE302: 'ne302_camera',
  NE503: 'ne503_camera',
  /** Product id is `ek410_Eink` (DeviceTypes / builtins) — not NK410. */
  EK410: 'ek410_Eink',
}

/** SoftAP discovery tokens accepted on DeviceTypes `supported_discovery`. */
function supportsApDiscovery(discovery: string[]): boolean {
  return discovery.some((d) => {
    const x = d.trim().toLowerCase()
    return x === 'ap' || x === 'softap'
  })
}

/** Plan / builtin SoftAP families when DeviceTypes are empty. */
const FALLBACK_AP_MODELS = ['NE101', 'NE301', 'NE302', 'NE503', 'EK410'] as const

export interface CtProvTypeMetadata {
  model_code?: number | null
  model_name?: string | null
  ap_ssid_families?: string[]
  softap_default_host?: string | null
  supported_discovery?: string[]
}

/** Minimal device-type shape needed for CT-Prov resolution (matches API DTO). */
export interface CtProvDeviceTypeRef {
  device_type: string
  name?: string
  ct_prov?: CtProvTypeMetadata | null
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

function normalizeModel(model: string): string {
  return model.trim().toUpperCase()
}

/** Family match: NE503C → NE503 when families list contains NE503. */
function familyMatch(model: string, family: string): boolean {
  const m = normalizeModel(model)
  const f = normalizeModel(family)
  return m === f || m.startsWith(f)
}

export function findCtProvType(
  deviceTypes: CtProvDeviceTypeRef[] | undefined | null,
  model: string,
): CtProvDeviceTypeRef | null {
  if (!deviceTypes?.length) return null
  const want = normalizeModel(model)

  for (const t of deviceTypes) {
    const name = t.ct_prov?.model_name
    if (name && normalizeModel(name) === want) return t
  }
  for (const t of deviceTypes) {
    const families = t.ct_prov?.ap_ssid_families ?? []
    if (families.some((f) => familyMatch(want, f))) return t
  }
  // device_type ne101_camera → NE101
  const guessed = `${want.toLowerCase()}_camera`
  return deviceTypes.find((t) => t.device_type === guessed) ?? null
}

/** Derive device type id from model, preferring DeviceTypes ct_prov metadata. */
export function modelToDeviceType(
  model: string,
  deviceTypes?: CtProvDeviceTypeRef[] | null,
): string | null {
  const fromMeta = findCtProvType(deviceTypes, model)
  if (fromMeta) return fromMeta.device_type

  const upper = normalizeModel(model)
  if (MODEL_TO_DEVICE_TYPE_FALLBACK[upper]) return MODEL_TO_DEVICE_TYPE_FALLBACK[upper]!
  if (/^[A-Z]+\d+[A-Z0-9]*$/.test(upper)) return `${upper.toLowerCase()}_camera`
  return null
}

/** SoftAP gateway host for a model family. */
export function softapHostForModel(
  model: string,
  deviceTypes?: CtProvDeviceTypeRef[] | null,
): string {
  const fromMeta = findCtProvType(deviceTypes, model)
  const host = fromMeta?.ct_prov?.softap_default_host?.trim()
  if (host) return host

  const upper = normalizeModel(model)
  for (const family of Object.keys(PLAN_SOFTAP_HOST_FALLBACK)) {
    if (upper === family || upper.startsWith(family)) {
      return PLAN_SOFTAP_HOST_FALLBACK[family]!
    }
  }
  return PLAN_SOFTAP_HOST_FALLBACK.NE101!
}

export function softapEndpointForHost(host: string): string {
  return `http://${host}/api/v1/ctprov`
}

/**
 * SoftAP model families for the AP tab dropdown.
 *
 * Always includes the seeded families (NE101/NE301/NE302/NE503/EK410).
 * DeviceTypes `ct_prov` overlays host/device_type when present — a store that
 * only has one SoftAP-capable type must NOT shrink the dropdown to a single
 * item (that was the live UX bug).
 */
export function listApProvisionModels(
  deviceTypes?: CtProvDeviceTypeRef[] | null,
): Array<{ model: string; host: string; deviceType: string }> {
  const fromTypes = new Map<string, { model: string; host: string; deviceType: string }>()
  if (deviceTypes?.length) {
    for (const t of deviceTypes) {
      const ct = t.ct_prov
      if (!ct) continue
      const discovery = ct.supported_discovery ?? []
      // Accept both `ap` and `softap` (EK410 DeviceTypes uses softap).
      if (discovery.length > 0 && !supportsApDiscovery(discovery)) continue
      const raw = (ct.model_name || ct.ap_ssid_families?.[0] || '').trim()
      if (!raw) continue
      const key = normalizeModel(raw)
      // Collapse NE503C → NE503 when it matches a seeded family.
      const family =
        FALLBACK_AP_MODELS.find((f) => key === f || key.startsWith(f)) ?? key
      fromTypes.set(family, {
        model: family,
        host: ct.softap_default_host?.trim() || softapHostForModel(family, deviceTypes),
        deviceType: t.device_type,
      })
    }
  }

  const seeded = FALLBACK_AP_MODELS.map((model) => {
    const hit = fromTypes.get(model)
    if (hit) return hit
    return {
      model,
      host: softapHostForModel(model, deviceTypes),
      deviceType: modelToDeviceType(model, deviceTypes)!,
    }
  })

  // Extra SoftAP families from DeviceTypes beyond the seeded five (rare).
  const extras = [...fromTypes.entries()]
    .filter(([k]) => !(FALLBACK_AP_MODELS as readonly string[]).includes(k))
    .map(([, v]) => v)

  return [...seeded, ...extras]
}

/** Resolve model string from BLE advertising model_code via DeviceTypes. */
export function modelNameFromCode(
  modelCode: number,
  deviceTypes?: CtProvDeviceTypeRef[] | null,
): string | undefined {
  if (deviceTypes?.length) {
    const hit = deviceTypes.find((t) => t.ct_prov?.model_code === modelCode)
    if (hit?.ct_prov?.model_name) return hit.ct_prov.model_name
  }
  // Plan / discovery fallbacks (also in discovery.MODEL_CODE_NAMES)
  if (modelCode === 0x01) return 'NE101'
  if (modelCode === 0x02) return 'NE301'
  if (modelCode === 0x03) return 'NE503'
  return undefined
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

/** @deprecated use MODEL_TO_DEVICE_TYPE_FALLBACK — kept for older imports */
export const MODEL_TO_DEVICE_TYPE = MODEL_TO_DEVICE_TYPE_FALLBACK
