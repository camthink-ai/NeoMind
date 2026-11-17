/**
 * CT-Prov GATT channel: scan / connect / META read / C2D write / D2C indicate.
 * Tauri uses @mnlphlp/plugin-blec; browser uses Web Bluetooth.
 */

import {
  CT_PROV_C2D_UUID,
  CT_PROV_D2C_UUID,
  CT_PROV_META_UUID,
  CT_PROV_SERVICE_UUID,
  normalizeBleUuid,
} from './constants'
import {
  applyScanResponse,
  parseAdvertisingData,
  parseMeta,
  type CtDiscoveredDevice,
  type CtMetaInfo,
} from './discovery'

export interface CtScanDevice {
  id: string
  name?: string
  rssi?: number
  discovered?: CtDiscoveredDevice
  /** True when CT Service UUID was observed on the advertisement. */
  hasCtService: boolean
}

export interface CtGattChannel {
  readonly mtu: number
  writeC2d(frame: Uint8Array): Promise<void>
  /** Subscribe once; handler receives each D2C ATT payload (with 5B fragment header). */
  subscribeD2c(handler: (frame: Uint8Array) => void): Promise<void>
  readMeta(): Promise<CtMetaInfo | null>
  disconnect(): void
}

function isTauriEnv(): boolean {
  return !!(window as unknown as { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__
}

const CT_SERVICE = normalizeBleUuid(CT_PROV_SERVICE_UUID)
const CT_C2D = normalizeBleUuid(CT_PROV_C2D_UUID)
const CT_D2C = normalizeBleUuid(CT_PROV_D2C_UUID)
const CT_META = normalizeBleUuid(CT_PROV_META_UUID)

function deviceHasCtService(d: {
  services?: string[]
  serviceData?: Record<string, number[]>
}): boolean {
  if (d.services?.some((s) => normalizeBleUuid(s) === CT_SERVICE)) return true
  if (d.serviceData) {
    return Object.keys(d.serviceData).some((k) => normalizeBleUuid(k) === CT_SERVICE)
  }
  return false
}

function tryParseAdvFromServiceData(
  serviceData: Record<string, number[]> | undefined,
): CtDiscoveredDevice | undefined {
  if (!serviceData) return undefined
  for (const [key, payload] of Object.entries(serviceData)) {
    if (normalizeBleUuid(key) !== CT_SERVICE) continue
    // WinRT/Apple: payload is the 6B discovery fields; Android may prepend UUID.
    // Rebuild full 27B AD for parseAdvertisingData.
    let fields = payload
    if (payload.length === 22) {
      // length includes UUID — take last 6
      fields = payload.slice(16)
    }
    if (fields.length < 6) continue
    const adv = [
      0x02, 0x01, 0x06, 0x17, 0x21,
      ...Array.from(
        // CT_PROV_SERVICE_UUID_LE
        [0x00, 0x01, 0x7e, 0x0b, 0x4c, 0x2a, 0x6d, 0x8f, 0x5b, 0x4b, 0x5b, 0x9d, 0x01, 0x00, 0x1e, 0x7b],
      ),
      ...fields.slice(0, 6),
    ]
    const parsed = parseAdvertisingData(adv)
    if (parsed) return parsed
  }
  return undefined
}

// ---------------------------------------------------------------------------
// Scan
// ---------------------------------------------------------------------------

export async function scanCtDevices(opts?: {
  durationMs?: number
}): Promise<CtScanDevice[]> {
  if (isTauriEnv()) {
    return scanTauri(opts?.durationMs ?? 10_000)
  }
  return scanWebBluetooth()
}

async function scanTauri(durationMs: number): Promise<CtScanDevice[]> {
  const { startScan, stopScan } = await import('@mnlphlp/plugin-blec')
  const devices: CtScanDevice[] = []
  const seen = new Set<string>()

  await startScan((found) => {
    for (const d of found) {
      if (seen.has(d.address)) continue
      const hasCt = deviceHasCtService(d)
      const fromAdv = tryParseAdvFromServiceData(d.serviceData)
      let discovered = fromAdv
      if (discovered && d.name) {
        const nameBytes = [1 + d.name.length, 0x09, ...Array.from(d.name, (c) => c.charCodeAt(0))]
        discovered = applyScanResponse(discovered, nameBytes)
      }
      // Accept CT service matches; also keep named NE* while migrating firmwares
      // are not yet advertising service data on every platform — but only if CT
      // service is present (hard cut: no legacy 0xFFFE path).
      if (!hasCt && !discovered) continue
      seen.add(d.address)
      devices.push({
        id: d.address,
        name: d.name || discovered?.displayName,
        rssi: d.rssi,
        discovered,
        hasCtService: hasCt || !!discovered,
      })
    }
  }, durationMs)

  await new Promise((r) => setTimeout(r, durationMs))
  await stopScan()
  devices.sort((a, b) => (b.rssi ?? -100) - (a.rssi ?? -100))
  return devices.filter((d) => d.hasCtService)
}

async function scanWebBluetooth(): Promise<CtScanDevice[]> {
  if (!navigator.bluetooth) {
    throw new Error('Web Bluetooth is not supported')
  }
  const selected = await navigator.bluetooth.requestDevice({
    filters: [{ services: [CT_SERVICE] }],
    optionalServices: [CT_SERVICE],
  })
  return [
    {
      id: selected.id,
      name: selected.name,
      hasCtService: true,
    },
  ]
}

// ---------------------------------------------------------------------------
// Channel implementations
// ---------------------------------------------------------------------------

class TauriCtGattChannel implements CtGattChannel {
  mtu = 23
  private d2cHandler: ((frame: Uint8Array) => void) | null = null

  async connect(deviceId: string, onDisconnect?: () => void): Promise<void> {
    const { connect } = await import('@mnlphlp/plugin-blec')
    await connect(deviceId, onDisconnect ?? null)
    // blec does not expose negotiated MTU; 23 is always safe for slicing.
    this.mtu = 23
  }

  async writeC2d(frame: Uint8Array): Promise<void> {
    const { send } = await import('@mnlphlp/plugin-blec')
    await send(CT_C2D, Array.from(frame), 'withResponse', CT_SERVICE)
  }

  async subscribeD2c(handler: (frame: Uint8Array) => void): Promise<void> {
    this.d2cHandler = handler
    const { subscribe } = await import('@mnlphlp/plugin-blec')
    await subscribe(CT_D2C, (data) => {
      this.d2cHandler?.(new Uint8Array(data))
    })
  }

  async readMeta(): Promise<CtMetaInfo | null> {
    const { read } = await import('@mnlphlp/plugin-blec')
    try {
      const bytes = await read(CT_META, CT_SERVICE)
      return parseMeta(bytes)
    } catch {
      return null
    }
  }

  disconnect(): void {
    import('@mnlphlp/plugin-blec')
      .then(async ({ unsubscribe, disconnect }) => {
        try {
          await unsubscribe(CT_D2C)
        } catch {
          /* ignore */
        }
        await disconnect()
      })
      .catch(() => {})
    this.d2cHandler = null
  }
}

class WebBluetoothCtGattChannel implements CtGattChannel {
  mtu = 23
  private bleDevice: BluetoothDevice | null = null
  private c2d: BluetoothRemoteGATTCharacteristic | null = null
  private d2c: BluetoothRemoteGATTCharacteristic | null = null
  private meta: BluetoothRemoteGATTCharacteristic | null = null
  private d2cHandler: ((frame: Uint8Array) => void) | null = null

  /** Web picker already selected the device during scan — reuse it. */
  attachSelectedDevice(device: BluetoothDevice): void {
    this.bleDevice = device
  }

  async connect(_deviceId: string, onDisconnect?: () => void): Promise<void> {
    if (!this.bleDevice) {
      // Re-request if scan did not stash the device (rare)
      if (!navigator.bluetooth) throw new Error('Web Bluetooth is not supported')
      this.bleDevice = await navigator.bluetooth.requestDevice({
        filters: [{ services: [CT_SERVICE] }],
        optionalServices: [CT_SERVICE],
      })
    }
    if (onDisconnect) {
      this.bleDevice.addEventListener('gattserverdisconnected', onDisconnect)
    }
    const gatt = this.bleDevice.gatt
    if (!gatt) throw new Error('No GATT server')
    if (gatt.connected) {
      try {
        gatt.disconnect()
      } catch {
        /* ignore */
      }
      await new Promise((r) => setTimeout(r, 500))
    }
    const server = await gatt.connect()
    const service = await server.getPrimaryService(CT_SERVICE)
    this.c2d = await service.getCharacteristic(CT_C2D)
    this.d2c = await service.getCharacteristic(CT_D2C)
    try {
      this.meta = await service.getCharacteristic(CT_META)
    } catch {
      this.meta = null
    }
  }

  async writeC2d(frame: Uint8Array): Promise<void> {
    if (!this.c2d) throw new Error('Not connected')
    await this.c2d.writeValueWithResponse(frame)
  }

  async subscribeD2c(handler: (frame: Uint8Array) => void): Promise<void> {
    if (!this.d2c) throw new Error('Not connected')
    this.d2cHandler = handler
    await this.d2c.startNotifications()
    this.d2c.addEventListener('characteristicvaluechanged', ((ev: Event) => {
      const target = ev.target as BluetoothRemoteGATTCharacteristic
      const view = target.value
      if (!view) return
      this.d2cHandler?.(new Uint8Array(view.buffer, view.byteOffset, view.byteLength))
    }) as EventListener)
  }

  async readMeta(): Promise<CtMetaInfo | null> {
    if (!this.meta) return null
    try {
      const view = await this.meta.readValue()
      return parseMeta(new Uint8Array(view.buffer, view.byteOffset, view.byteLength))
    } catch {
      return null
    }
  }

  disconnect(): void {
    try {
      this.bleDevice?.gatt?.disconnect()
    } catch {
      /* ignore */
    }
    if (lastWebDevice && this.bleDevice && lastWebDevice.id === this.bleDevice.id) {
      lastWebDevice = null
    }
    this.bleDevice = null
    this.c2d = null
    this.d2c = null
    this.meta = null
    this.d2cHandler = null
  }
}

/** Hold the last WebBluetooth device from scan so connect can reuse it. */
let lastWebDevice: BluetoothDevice | null = null

export async function scanCtDevicesWithWebStash(
  opts?: { durationMs?: number },
): Promise<CtScanDevice[]> {
  if (!isTauriEnv() && navigator.bluetooth) {
    const selected = await navigator.bluetooth.requestDevice({
      filters: [{ services: [CT_SERVICE] }],
      optionalServices: [CT_SERVICE],
    })
    lastWebDevice = selected
    return [
      {
        id: selected.id,
        name: selected.name,
        hasCtService: true,
      },
    ]
  }
  return scanCtDevices(opts)
}

export async function openCtGattChannel(
  deviceId: string,
  onDisconnect?: () => void,
): Promise<CtGattChannel> {
  if (isTauriEnv()) {
    const ch = new TauriCtGattChannel()
    await ch.connect(deviceId, onDisconnect)
    return ch
  }
  const ch = new WebBluetoothCtGattChannel()
  if (lastWebDevice && lastWebDevice.id === deviceId) {
    ch.attachSelectedDevice(lastWebDevice)
  }
  await ch.connect(deviceId, onDisconnect)
  return ch
}

export { isTauriEnv }
