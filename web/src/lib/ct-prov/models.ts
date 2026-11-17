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

/**
 * MQTT config returned by POST /devices/ble-provision (resolve phase).
 *
 * CT-Prov/1 mqtt section schema (neolink golden capabilities):
 * enable, host, port, client_id, username, password, tls, topic, qos.
 * keep_alive / clean_session / will / alpn are NOT in the protocol — omit.
 */
export interface CtMqttConfig {
  host: string
  port: number
  username: string
  password: string
  topic_prefix: string
  client_id: string
  /** Explicit TLS; when omitted, inferred from port === 8883. */
  tls?: boolean
  /** MQTT enable flag; defaults to true on write. */
  enable?: boolean
  /** Publish/subscribe QoS 0–2; defaults to 1 on write. */
  qos?: number
}

/** Device-custom broker fields sent as `custom_mqtt` on ble-provision. */
export interface CustomMqttFields {
  host: string
  port: number
  username?: string
  password?: string
  tls?: boolean
  enable?: boolean
  qos?: number
  topic_prefix?: string
  client_id?: string
}

/** CT-Prov/1 mqtt section field names (capabilities.fields order). */
export const CT_MQTT_SECTION_FIELDS = [
  'enable',
  'host',
  'port',
  'client_id',
  'username',
  'password',
  'tls',
  'topic',
  'qos',
] as const

/**
 * CT-Prov/1 cellular section (neolink golden capabilities).
 * NE101 omits `roaming`; NE301 may declare it.
 */
export const CT_CELLULAR_SECTION_FIELDS = [
  'apn',
  'username',
  'password',
  'pin',
  'auth',
  'roaming',
] as const

/** CT-Prov/1 webhook section (neolink golden capabilities). */
export const CT_WEBHOOK_SECTION_FIELDS = ['enable', 'url', 'secret', 'header'] as const

export type CtMqttQos = 0 | 1 | 2

export type CellularAuth = 'none' | 'pap' | 'chap' | 'pap_or_chap'

export const CELLULAR_AUTH_VALUES: readonly CellularAuth[] = [
  'none',
  'pap',
  'chap',
  'pap_or_chap',
] as const

export interface CellularFields {
  apn?: string
  username?: string
  password?: string
  pin?: string
  auth?: CellularAuth
  roaming?: boolean
}

export interface WebhookFields {
  enable: boolean
  url: string
  secret?: string
  header?: string
}

export function normalizeMqttQos(value: unknown, fallback: CtMqttQos = 1): CtMqttQos {
  const n = typeof value === 'number' ? value : Number(value)
  if (n === 0 || n === 1 || n === 2) return n
  return fallback
}

export function normalizeCellularAuth(
  value: unknown,
  fallback: CellularAuth = 'none',
): CellularAuth {
  if (typeof value === 'string' && (CELLULAR_AUTH_VALUES as readonly string[]).includes(value)) {
    return value as CellularAuth
  }
  return fallback
}

function fieldDeclared(declared: readonly string[] | undefined, field: string): boolean {
  return !declared || declared.includes(field)
}

/**
 * Build CT `cellular` section gated by capabilities.fields.
 * Returns null when there is nothing to write (all empty / undeclared).
 *
 * Wi‑Fi-only Apply: if capabilities declare `apn` but the operator left it
 * blank, return null — do NOT emit a partial object like `{auth:"none"}`.
 * NE101 rejects that with INVALID_ARGUMENT "cellular.apn: required field missing".
 * Pre-provisioned SIMs that omit `apn` from capabilities.fields still work.
 */
export function cellularSectionFromFields(
  fields: CellularFields,
  declared?: readonly string[],
): Record<string, unknown> | null {
  const apn = fields.apn?.trim() ?? ''
  // Declared APN left empty → skip the whole cellular write (Wi‑Fi path).
  if (fieldDeclared(declared, 'apn') && !apn) {
    return null
  }

  const out: Record<string, unknown> = {}
  if (apn && fieldDeclared(declared, 'apn')) out.apn = apn
  const username = fields.username?.trim()
  if (username && fieldDeclared(declared, 'username')) out.username = username
  if (fields.password && fieldDeclared(declared, 'password')) out.password = fields.password
  const pin = fields.pin?.trim()
  if (pin && fieldDeclared(declared, 'pin')) out.pin = pin
  if (fields.auth && fieldDeclared(declared, 'auth')) out.auth = fields.auth
  if (fields.roaming != null && fieldDeclared(declared, 'roaming')) out.roaming = fields.roaming

  // Default auth=none alone is not a real cellular configuration.
  if (Object.keys(out).length === 1 && out.auth === 'none') {
    return null
  }
  return Object.keys(out).length > 0 ? out : null
}

/**
 * Build CT `webhook` section gated by capabilities.fields.
 * enable=false is a complete write (device clears outbound webhook).
 */
export function webhookSectionFromFields(
  fields: WebhookFields,
  declared?: readonly string[],
): Record<string, unknown> | null {
  if (!fieldDeclared(declared, 'enable')) return null
  const out: Record<string, unknown> = { enable: fields.enable }
  const url = fields.url.trim()
  if (url && fieldDeclared(declared, 'url')) out.url = url
  const secret = fields.secret?.trim()
  if (secret && fieldDeclared(declared, 'secret')) out.secret = secret
  const header = fields.header?.trim()
  if (header && fieldDeclared(declared, 'header')) out.header = header
  return out
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

/**
 * Build CT `mqtt` section from NeoMind resolve response.
 *
 * Firmware treats `topic` as the **exact publish topic** (not a prefix).
 * NeoMind registers/subscribes `device/{type}/{id}/uplink`, so we must write
 * the full uplink path — writing only the prefix caused devices to publish
 * `device/{type}/{id}` and auto-discovery to mis-parse device_id as the type.
 */
export function mqttSectionFromResolve(cfg: CtMqttConfig): Record<string, unknown> {
  const prefix = (cfg.topic_prefix || '').replace(/\/+$/, '')
  const topic =
    !prefix
      ? undefined
      : prefix.endsWith('/uplink')
        ? prefix
        : `${prefix}/uplink`
  // Write the full CT-Prov/1 mqtt section (all capabilities.fields keys).
  // Empty username/password stay omitted so firmware optional-auth stays clean.
  return {
    enable: cfg.enable ?? true,
    host: cfg.host,
    port: cfg.port,
    client_id: cfg.client_id,
    username: cfg.username || undefined,
    password: cfg.password || undefined,
    tls: cfg.tls ?? cfg.port === 8883,
    topic,
    qos: normalizeMqttQos(cfg.qos, 1),
  }
}

/** @deprecated use MODEL_TO_DEVICE_TYPE_FALLBACK — kept for older imports */
export const MODEL_TO_DEVICE_TYPE = MODEL_TO_DEVICE_TYPE_FALLBACK
