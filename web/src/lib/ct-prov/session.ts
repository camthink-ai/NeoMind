/**
 * CT-Prov session client: envelope-level orchestration over ProvTransport.
 * Path for M0: device.info.get → communication.capabilities.get → config.set → session.close.
 */

import { DEFAULT_REQUEST_TIMEOUT_MS } from './constants'
import type { JsonObject, ProvRequest, ProvResponse } from './envelope'
import { ProvErrorCode, ProvOp, resultOpName } from './ops'
import type { ProvTransport } from './transport'

export class ProvSessionError extends Error {
  readonly reason: string

  constructor(reason: string, message?: string) {
    super(message ? `ProvSessionError(${reason}: ${message})` : `ProvSessionError(${reason})`)
    this.name = 'ProvSessionError'
    this.reason = reason
  }
}

/**
 * Map protocol / transport errors to a short operator-facing sentence.
 * Keeps the raw `ProvSessionError(...)` string out of the wizard UI.
 */
export function formatProvUserMessage(err: unknown): string {
  const raw = err instanceof Error ? err.message : String(err)
  const lower = raw.toLowerCase()
  if (lower.includes('wifi_init')) {
    return 'wifi_init_failed'
  }
  // Device tried to join the SSID and failed (wrong password / out of range / …).
  if (
    lower.includes('wifi associate') ||
    lower.includes('associate failed') ||
    (lower.includes('storage_failed') && lower.includes('wifi'))
  ) {
    return 'wifi_associate_failed'
  }
  if (lower.includes('cellular.apn') || lower.includes('cellular apn')) {
    return 'cellular_apn_required'
  }
  if (lower.includes('storage_failed')) {
    return 'storage_failed'
  }
  if (lower.includes('busy')) {
    return 'device_busy'
  }
  if (lower.includes('timed out') || lower.includes('timeout')) {
    return 'request_timeout'
  }
  if (err instanceof ProvSessionError) {
    if (err.reason === 'internal_error' || lower.includes('internal_error')) {
      return 'internal_error'
    }
    if (err.reason === 'not_open') {
      return 'not_connected'
    }
    if (err.reason === 'storage_failed') {
      return 'storage_failed'
    }
  }
  if (lower.includes('not connected') || lower.includes('not_open')) {
    return 'not_connected'
  }
  // Fall back to a cleaned reason/message without the ProvSessionError wrapper.
  const m = raw.match(/^ProvSessionError\((.+)\)$/)
  return m?.[1] ?? raw
}

export interface DeviceInfoResult {
  deviceName: string
  serialNumber: string
  model: string
  firmware: string
  power: string[]
  protocolVersion: number
}

export type ConfigSectionKind = 'network' | 'selector' | 'service'

export interface ConfigSection {
  name: string
  kind: ConfigSectionKind
  configurable: boolean
  scannable: boolean
  fields: string[]
}

export interface CapabilitiesResult {
  sections: ConfigSection[]
  currentNetwork: string | null
}

export interface SetConfigResult {
  stored: string[]
  apply?: string
}

/** One Wi-Fi / HaLow network from `communication.scan`. */
export interface NetworkScanItem {
  /**
   * Submit / select value = CT-Prov `network_id` (SSID for wifi/halow).
   * Leading/trailing spaces are significant — do not trim for provision.
   */
  ssid: string
  /** Operator-facing label (`display_name`); falls back to `ssid`. */
  displayName?: string
  /** dBm — from wire `signal` (golden) or legacy `rssi`. */
  rssi?: number
  bssid?: string
  security?: string
  channel?: number
}

/** Display helper: keep submit value (`item.ssid`) untrimmed; UI may format. */
export function formatSsidForDisplay(ssid: string): string {
  return ssid
}

export interface NetworkScanPage {
  section: string
  items: NetworkScanItem[]
  more: boolean
  cursor: string | null
}

function sectionKindFromWire(name: string): ConfigSectionKind | null {
  if (name === 'network' || name === 'selector' || name === 'service') return name
  return null
}

export function parseDeviceInfo(response: ProvResponse): DeviceInfoResult {
  if (!response.ok) {
    throw new ProvSessionError(
      'device_info_error',
      `${response.error?.codeName}: ${response.error?.message}`,
    )
  }
  if (response.opName !== resultOpName(ProvOp.deviceInfoGet)) {
    throw new ProvSessionError('device_info_op', response.opName)
  }
  const data = response.data
  const deviceName = data?.device_name
  const serialNumber = data?.serial_number
  const model = data?.model
  const firmware = data?.firmware
  const power = data?.power
  const protocolVersion = data?.protocol_version
  if (
    typeof deviceName !== 'string' ||
    typeof serialNumber !== 'string' ||
    typeof model !== 'string' ||
    typeof firmware !== 'string'
  ) {
    throw new ProvSessionError(
      'device_info_shape',
      'device_name/serial_number/model/firmware must be strings',
    )
  }
  if (!Array.isArray(power) || power.some((p) => typeof p !== 'string')) {
    throw new ProvSessionError('device_info_shape', 'power must be an array of strings')
  }
  if (typeof protocolVersion !== 'number' || protocolVersion !== 1) {
    throw new ProvSessionError('device_info_version', `protocol_version=${protocolVersion}`)
  }
  return {
    deviceName,
    serialNumber,
    model,
    firmware,
    power: power as string[],
    protocolVersion,
  }
}

export function parseCapabilities(response: ProvResponse): CapabilitiesResult {
  if (!response.ok) {
    throw new ProvSessionError(
      'capabilities_error',
      `${response.error?.codeName}: ${response.error?.message}`,
    )
  }
  // Firmware uses …get.result; protocol doc examples drop the verb → …capabilities.result.
  const accepted = new Set([
    'communication.capabilities.result',
    'communication.capabilities.get.result',
  ])
  if (!accepted.has(response.opName)) {
    throw new ProvSessionError('capabilities_op', response.opName)
  }
  const sectionsRaw = response.data?.sections
  if (!Array.isArray(sectionsRaw)) {
    throw new ProvSessionError('capabilities_shape', 'sections must be an array')
  }
  const sections: ConfigSection[] = sectionsRaw.map((entry) => parseSection(entry))
  let currentNetwork: string | null = null
  const current = response.data?.current
  if (current && typeof current === 'object' && !Array.isArray(current)) {
    const network = (current as JsonObject).network
    if (network !== null && network !== undefined && typeof network !== 'string') {
      throw new ProvSessionError('capabilities_shape', 'current.network must be a string or null')
    }
    currentNetwork = (network as string | null | undefined) ?? null
  }
  return { sections, currentNetwork }
}

function parseSection(entry: unknown): ConfigSection {
  if (!entry || typeof entry !== 'object' || Array.isArray(entry)) {
    throw new ProvSessionError('capabilities_shape', 'section entry must be an object')
  }
  const obj = entry as JsonObject
  const name = obj.name
  const kindName = obj.kind
  const configurable = obj.configurable
  const scannable = obj.scannable
  const fields = obj.fields
  if (typeof name !== 'string' || typeof kindName !== 'string') {
    throw new ProvSessionError('capabilities_shape', 'section name/kind must be strings')
  }
  const kind = sectionKindFromWire(kindName)
  if (!kind) {
    throw new ProvSessionError('capabilities_shape', `unknown kind: ${kindName}`)
  }
  if (typeof configurable !== 'boolean') {
    throw new ProvSessionError('capabilities_shape', 'configurable must be a boolean')
  }
  if (fields == null) {
    return {
      name,
      kind,
      configurable,
      scannable: typeof scannable === 'boolean' ? scannable : false,
      fields: [],
    }
  }
  if (!Array.isArray(fields) || fields.some((f) => typeof f !== 'string')) {
    throw new ProvSessionError('capabilities_shape', 'fields must be an array of strings')
  }
  return {
    name,
    kind,
    configurable,
    scannable: typeof scannable === 'boolean' ? scannable : false,
    fields: fields as string[],
  }
}

export function parseNetworkScan(response: ProvResponse): NetworkScanPage {
  if (!response.ok) {
    throw new ProvSessionError(
      response.error?.codeName?.toLowerCase() ?? 'scan_error',
      `${response.error?.codeName}: ${response.error?.message}`,
    )
  }
  const accepted = new Set([
    resultOpName(ProvOp.communicationScan),
    'communication.scan.result',
  ])
  if (!accepted.has(response.opName)) {
    throw new ProvSessionError('scan_op', response.opName)
  }
  const data = response.data ?? {}
  const section = typeof data.section === 'string' ? data.section : 'wifi'
  const rawItems = data.items
  if (!Array.isArray(rawItems)) {
    throw new ProvSessionError('scan_shape', 'items must be an array')
  }
  const items: NetworkScanItem[] = []
  for (const entry of rawItems) {
    if (!entry || typeof entry !== 'object' || Array.isArray(entry)) continue
    const obj = entry as JsonObject
    // CT-Prov/1 golden (§9): network_id + display_name + signal.
    // Legacy / FakeTransport may still send ssid + rssi — accept both.
    const networkId = obj.network_id
    const legacySsid = obj.ssid
    const ssid =
      typeof networkId === 'string' && networkId.length > 0
        ? networkId
        : typeof legacySsid === 'string' && legacySsid.length > 0
          ? legacySsid
          : null
    // Empty string only — leading/trailing spaces are part of the real SSID
    // and must be preserved for provision submit (do not trim).
    if (ssid == null) continue
    const item: NetworkScanItem = { ssid }
    const displayName = obj.display_name
    if (typeof displayName === 'string' && displayName.length > 0) {
      item.displayName = displayName
    }
    if (typeof obj.signal === 'number') item.rssi = obj.signal
    else if (typeof obj.rssi === 'number') item.rssi = obj.rssi
    if (typeof obj.channel === 'number') item.channel = obj.channel
    if (typeof obj.bssid === 'string') item.bssid = obj.bssid
    if (typeof obj.security === 'string') item.security = obj.security
    items.push(item)
  }
  const more = data.more === true
  const cursor =
    typeof data.cursor === 'string' && data.cursor.length > 0 ? data.cursor : null
  return { section, items, more, cursor }
}

export function parseSetConfig(response: ProvResponse): SetConfigResult {
  if (!response.ok) {
    throw new ProvSessionError(
      response.error?.codeName?.toLowerCase() ?? 'config_set_error',
      `${response.error?.codeName}: ${response.error?.message}`,
    )
  }
  if (response.opName !== resultOpName(ProvOp.configSet)) {
    throw new ProvSessionError('config_set_op', response.opName)
  }
  const stored = response.data?.stored
  if (!Array.isArray(stored) || stored.some((s) => typeof s !== 'string')) {
    throw new ProvSessionError('config_set_shape', 'stored must be an array of strings')
  }
  const apply = response.data?.apply
  if (apply !== undefined && apply !== null && typeof apply !== 'string') {
    throw new ProvSessionError('config_set_shape', 'apply must be a string')
  }
  return {
    stored: stored as string[],
    apply: typeof apply === 'string' ? apply : undefined,
  }
}

export interface ProvisioningSessionResult {
  deviceInfo: DeviceInfoResult
  capabilities: CapabilitiesResult
  configSet: SetConfigResult
}

/**
 * Envelope-level CT-Prov session client.
 * Request ids are allocated here; fragment message ids belong to the transport.
 */
export class CtSessionClient {
  private transport: ProvTransport | null
  private nextRequestId = 1
  private opened = false

  constructor(transport: ProvTransport) {
    this.transport = transport
  }

  async open(): Promise<void> {
    this.opened = true
  }

  private requireTransport(): ProvTransport {
    if (!this.opened || !this.transport) {
      throw new ProvSessionError('not_open', 'call open() before sending requests')
    }
    return this.transport
  }

  private nextRequest(op: string, data: JsonObject = {}): ProvRequest {
    return {
      kind: 'request',
      id: this.nextRequestId++,
      opName: op,
      data,
      op: null,
    }
  }

  async fetchDeviceInfo(timeoutMs = DEFAULT_REQUEST_TIMEOUT_MS): Promise<DeviceInfoResult> {
    const response = await this.requireTransport().sendRequest(
      this.nextRequest(ProvOp.deviceInfoGet),
      { timeoutMs },
    )
    return parseDeviceInfo(response)
  }

  async fetchCapabilities(
    timeoutMs = DEFAULT_REQUEST_TIMEOUT_MS,
  ): Promise<CapabilitiesResult> {
    const response = await this.requireTransport().sendRequest(
      this.nextRequest(ProvOp.communicationCapabilitiesGet),
      { timeoutMs },
    )
    return parseCapabilities(response)
  }

  /**
   * Scan networks for a scannable section (wifi / halow). Follows `more`+`cursor`
   * until complete or `maxPages` (devices can keep paging forever).
   */
  async scanNetworks(
    section: string,
    opts?: { timeoutMs?: number; maxPages?: number },
  ): Promise<NetworkScanItem[]> {
    const timeoutMs = opts?.timeoutMs ?? DEFAULT_REQUEST_TIMEOUT_MS
    const maxPages = opts?.maxPages ?? 8
    const seen = new Set<string>()
    const out: NetworkScanItem[] = []
    let cursor: string | undefined
    for (let page = 0; page < maxPages; page++) {
      const data: JsonObject = { section }
      if (cursor) data.cursor = cursor
      const response = await this.requireTransport().sendRequest(
        this.nextRequest(ProvOp.communicationScan, data),
        { timeoutMs },
      )
      const parsed = parseNetworkScan(response)
      for (const item of parsed.items) {
        const key = item.bssid ? `${item.ssid}\0${item.bssid}` : item.ssid
        if (seen.has(key)) continue
        seen.add(key)
        out.push(item)
      }
      if (!parsed.more || !parsed.cursor) break
      cursor = parsed.cursor
    }
    out.sort((a, b) => (b.rssi ?? -200) - (a.rssi ?? -200))
    return out
  }

  async setConfig(
    sections: Record<string, JsonObject>,
    timeoutMs = DEFAULT_REQUEST_TIMEOUT_MS,
  ): Promise<SetConfigResult> {
    const response = await this.requireTransport().sendRequest(
      this.nextRequest(ProvOp.configSet, sections),
      { timeoutMs },
    )
    return parseSetConfig(response)
  }

  async getConfig(
    timeoutMs = DEFAULT_REQUEST_TIMEOUT_MS,
  ): Promise<Record<string, JsonObject> | null> {
    const response = await this.requireTransport().sendRequest(
      this.nextRequest(ProvOp.configGet),
      { timeoutMs },
    )
    if (!response.ok) {
      const code = response.error?.codeName ?? 'UNKNOWN'
      if (code === ProvErrorCode.unsupportedOperation) return null
      throw new ProvSessionError(code.toLowerCase(), `${code}: ${response.error?.message ?? ''}`)
    }
    if (response.opName !== resultOpName(ProvOp.configGet)) {
      throw new ProvSessionError('config_get_op', response.opName)
    }
    const data = response.data
    if (!data || Object.values(data).some((v) => !v || typeof v !== 'object' || Array.isArray(v))) {
      throw new ProvSessionError('config_get_shape', 'data must be a map of section objects')
    }
    const out: Record<string, JsonObject> = {}
    for (const [k, v] of Object.entries(data)) {
      out[k] = v as JsonObject
    }
    return out
  }

  async closeSession(timeoutMs = DEFAULT_REQUEST_TIMEOUT_MS): Promise<void> {
    const response = await this.requireTransport().sendRequest(
      this.nextRequest(ProvOp.sessionClose),
      { timeoutMs },
    )
    if (!response.ok) {
      throw new ProvSessionError('close_rejected', response.error?.codeName)
    }
  }

  async dispose(): Promise<void> {
    const transport = this.transport
    this.transport = null
    this.opened = false
    await transport?.close()
  }

  /**
   * Happy-path provisioning sequence required by M0 exit criteria.
   * Does not register with the NeoMind platform (that stays in the API layer).
   */
  async runProvisioningPath(
    sections: Record<string, JsonObject>,
  ): Promise<ProvisioningSessionResult> {
    await this.open()
    const deviceInfo = await this.fetchDeviceInfo()
    const capabilities = await this.fetchCapabilities()
    const configSet = await this.setConfig(sections)
    await this.closeSession()
    return { deviceInfo, capabilities, configSet }
  }
}
