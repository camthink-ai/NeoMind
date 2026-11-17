/**
 * CamThink CT-Prov provision hook — BLE GATT or SoftAP HTTP share the same session path.
 */

import { useCallback, useRef, useState } from 'react'

import {
  BleGattCtTransport,
  CtSessionClient,
  SoftapHttpCtTransport,
  SoftapTransportError,
  isTauriEnv,
  mqttSectionFromResolve,
  CONFIG_SET_TIMEOUT_MS,
  CT_PROV_BLE_SCAN_MS,
  openCtGattChannel,
  parseApSsid,
  pruneIncompleteConfigSections,
  retainActiveBleSession,
  scanCtDevicesWithWebStash,
  softapFailureKind,
  softapHostForModel,
  type CapabilitiesResult,
  type CtMqttConfig,
  type CtScanDevice,
  type DeviceInfoResult,
  type JsonObject,
  type NetworkScanItem,
  type ProvTransport,
  type SetConfigResult,
} from '@/lib/ct-prov'

export type CtProvisionChannel = 'ble' | 'ap'

export interface UseCtProvisionReturn {
  channel: CtProvisionChannel
  scanning: boolean
  connecting: boolean
  provisioning: boolean
  connected: boolean
  error: string | null
  scannedDevices: CtScanDevice[]
  deviceInfo: DeviceInfoResult | null
  capabilities: CapabilitiesResult | null
  selectedScan: CtScanDevice | null
  provisioningStep: string | null
  /** SoftAP: detected / chosen model hint */
  apModelHint: string | null
  apHost: string
  apSsidInput: string

  checkAdapter: () => Promise<'ok' | 'off' | 'unavailable'>
  scan: () => Promise<CtScanDevice[]>
  selectBleDevice: (device: CtScanDevice) => Promise<boolean>
  /** SoftAP: set SSID (optional) + model/host, then open HTTP session. */
  connectAp: (opts: {
    ssid?: string
    model?: string
    host?: string
  }) => Promise<{ ok: true } | { ok: false; error: string; kind: ReturnType<typeof softapFailureKind> }>
  setApSsidInput: (v: string) => void
  setApHost: (v: string) => void
  setApModelHint: (v: string | null) => void
  /** Run config.set with wifi/cellular/webhook (+ mqtt from resolve) then session.close. */
  applyConfig: (opts: {
    wifiSsid?: string
    wifiPassword?: string
    mqtt: CtMqttConfig
    networkSection?: string
    /** CT cellular section when capabilities declare it configurable. */
    cellular?: Record<string, unknown>
    /** CT webhook section when capabilities declare it configurable. */
    webhook?: Record<string, unknown>
  }) => Promise<SetConfigResult>
  /** Device-side Wi-Fi / HaLow scan when the section is scannable. */
  scanWifiNetworks: (section?: string) => Promise<NetworkScanItem[]>
  disconnect: () => void
  clearError: () => void
}

function withTimeout<T>(promise: Promise<T>, ms: number, label: string): Promise<T> {
  return new Promise<T>((resolve, reject) => {
    const timer = setTimeout(() => reject(new Error(`${label} timed out after ${ms}ms`)), ms)
    promise.then(
      (v) => {
        clearTimeout(timer)
        resolve(v)
      },
      (e) => {
        clearTimeout(timer)
        reject(e)
      },
    )
  })
}

export function useCtProvision(channel: CtProvisionChannel): UseCtProvisionReturn {
  const [scanning, setScanning] = useState(false)
  const [connecting, setConnecting] = useState(false)
  const [provisioning, setProvisioning] = useState(false)
  const [connected, setConnected] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [scannedDevices, setScannedDevices] = useState<CtScanDevice[]>([])
  const [deviceInfo, setDeviceInfo] = useState<DeviceInfoResult | null>(null)
  const [capabilities, setCapabilities] = useState<CapabilitiesResult | null>(null)
  const [selectedScan, setSelectedScan] = useState<CtScanDevice | null>(null)
  const [provisioningStep, setProvisioningStep] = useState<string | null>(null)
  const [apModelHint, setApModelHint] = useState<string | null>(null)
  const [apHost, setApHost] = useState(softapHostForModel('NE101'))
  const [apSsidInput, setApSsidInput] = useState('')

  const clientRef = useRef<CtSessionClient | null>(null)
  const transportRef = useRef<ProvTransport | null>(null)
  /** Bumped on disconnect/cancel so in-flight scan/connect ignore late results. */
  const opGenRef = useRef(0)
  /** Unregister from the process-wide BLE session registry (does not release). */
  const clearBleRetainRef = useRef<(() => void) | null>(null)
  /** Stable disconnect entry for retainActiveBleSession (avoids stale closures). */
  const disconnectImplRef = useRef<() => void>(() => {})

  const clearError = useCallback(() => setError(null), [])

  const clearBleRetain = useCallback(() => {
    clearBleRetainRef.current?.()
    clearBleRetainRef.current = null
  }, [])

  const disposeSession = useCallback(async () => {
    try {
      await clientRef.current?.dispose()
    } catch {
      /* ignore */
    }
    clientRef.current = null
    transportRef.current = null
  }, [])

  const checkAdapter = useCallback(async (): Promise<'ok' | 'off' | 'unavailable'> => {
    if (channel === 'ap') return 'ok'
    if (!isTauriEnv() && !navigator.bluetooth) return 'unavailable'
    if (isTauriEnv()) {
      try {
        const { getAdapterState } = await import('@mnlphlp/plugin-blec')
        const state = await getAdapterState()
        if (state === 'Off') return 'off'
      } catch {
        return 'unavailable'
      }
    }
    return 'ok'
  }, [channel])

  const scan = useCallback(async (): Promise<CtScanDevice[]> => {
    const gen = ++opGenRef.current
    setScanning(true)
    setError(null)
    setScannedDevices([])
    await new Promise((r) => setTimeout(r, 0))
    try {
      const devices = await scanCtDevicesWithWebStash({
        durationMs: CT_PROV_BLE_SCAN_MS,
        // Progressive list so a 30s window can show multiple devices as they appear.
        onDevices: (partial) => {
          if (gen !== opGenRef.current) return
          setScannedDevices(partial)
        },
      })
      if (gen !== opGenRef.current) return []
      setScannedDevices(devices)
      return devices
    } catch (err: unknown) {
      if (gen !== opGenRef.current) return []
      if (err instanceof DOMException && err.name === 'NotFoundError') {
        setScannedDevices([])
        return []
      }
      setError(err instanceof Error ? err.message : String(err))
      return []
    } finally {
      if (gen === opGenRef.current) setScanning(false)
    }
  }, [])

  const openWithTransport = useCallback(
    async (transport: ProvTransport): Promise<boolean> => {
      await disposeSession()
      transportRef.current = transport
      if (transport instanceof BleGattCtTransport) {
        await transport.start()
      }
      const client = new CtSessionClient(transport)
      await client.open()
      clientRef.current = client

      const info = await withTimeout(client.fetchDeviceInfo(), 15_000, 'device.info.get')
      setDeviceInfo(info)
      const caps = await withTimeout(client.fetchCapabilities(), 15_000, 'capabilities.get')
      setCapabilities(caps)
      setConnected(true)

      // BLE only: advertise this GATT session so route / dialog guards can drop it.
      if (channel === 'ble' && transport instanceof BleGattCtTransport) {
        clearBleRetain()
        clearBleRetainRef.current = retainActiveBleSession(() => {
          disconnectImplRef.current()
        })
      }
      return true
    },
    [channel, clearBleRetain, disposeSession],
  )

  const selectBleDevice = useCallback(
    async (device: CtScanDevice): Promise<boolean> => {
      const gen = ++opGenRef.current
      // Selecting mid-scan cancels progressive updates (gen bump) and clears spinner.
      setScanning(false)
      setSelectedScan(device)
      setConnecting(true)
      setError(null)
      setDeviceInfo(null)
      setCapabilities(null)
      try {
        if (device.discovered?.busy) {
          throw new Error('Device is busy and not provisionable')
        }
        if (device.discovered && device.discovered.provisionable === false) {
          throw new Error('Device is not in provisionable mode')
        }
        const channelGatt = await withTimeout(
          openCtGattChannel(device.id, () => {
            setConnected(false)
          }),
          30_000,
          'BLE connect',
        )
        if (gen !== opGenRef.current) {
          channelGatt.disconnect()
          return false
        }
        const meta = await channelGatt.readMeta()
        if (gen !== opGenRef.current) {
          channelGatt.disconnect()
          return false
        }
        if (meta?.busy) {
          channelGatt.disconnect()
          throw new Error('Device META reports BUSY')
        }
        const transport = new BleGattCtTransport(channelGatt)
        return await openWithTransport(transport)
      } catch (err: unknown) {
        if (gen !== opGenRef.current) return false
        setError(err instanceof Error ? err.message : String(err))
        setConnected(false)
        await disposeSession()
        return false
      } finally {
        if (gen === opGenRef.current) setConnecting(false)
      }
    },
    [disposeSession, openWithTransport],
  )

  const connectAp = useCallback(
    async (opts: {
      ssid?: string
      model?: string
      host?: string
    }): Promise<
      { ok: true } | { ok: false; error: string; kind: ReturnType<typeof softapFailureKind> }
    > => {
      setConnecting(true)
      setError(null)
      setDeviceInfo(null)
      setCapabilities(null)
      try {
        let model = opts.model ?? apModelHint ?? undefined
        let host = (opts.host ?? apHost).trim()
        const ssid = opts.ssid ?? apSsidInput
        if (ssid) {
          const parsed = parseApSsid(ssid.trim())
          if (parsed?.modelHint) {
            model = parsed.modelHint
            setApModelHint(parsed.modelHint)
          }
          if (!opts.host && model) {
            host = softapHostForModel(model)
            setApHost(host)
          }
        }
        if (opts.model) {
          setApModelHint(opts.model)
          if (!opts.host) {
            host = softapHostForModel(opts.model)
            setApHost(host)
          }
        }
        if (!host && model) {
          host = softapHostForModel(model)
          setApHost(host)
        }
        const transport = new SoftapHttpCtTransport({
          host: host || undefined,
          model: model ?? undefined,
        })
        const opened = await openWithTransport(transport)
        if (!opened) {
          const msg = 'Failed to open SoftAP session'
          setError(msg)
          return { ok: false, error: msg, kind: 'other' }
        }
        return { ok: true }
      } catch (err: unknown) {
        const msg =
          err instanceof SoftapTransportError || err instanceof Error
            ? err.message
            : String(err)
        setError(msg)
        setConnected(false)
        await disposeSession()
        return { ok: false, error: msg, kind: softapFailureKind(err) }
      } finally {
        setConnecting(false)
      }
    },
    [apHost, apModelHint, apSsidInput, disposeSession, openWithTransport],
  )

  const scanWifiNetworks = useCallback(
    async (section = 'wifi'): Promise<NetworkScanItem[]> => {
      const client = clientRef.current
      if (!client) {
        throw new Error('Not connected')
      }
      // Do not set session-level `error` here — scan is optional. A failed
      // communication.scan (e.g. wifi_init) must not poison the apply path or
      // leave a raw ProvSessionError under the configure form.
      return await withTimeout(
        client.scanNetworks(section),
        25_000,
        'communication.scan',
      )
    },
    [],
  )

  const applyConfig = useCallback(
    async (opts: {
      wifiSsid?: string
      wifiPassword?: string
      mqtt: CtMqttConfig
      networkSection?: string
      cellular?: Record<string, unknown>
      webhook?: Record<string, unknown>
    }): Promise<SetConfigResult> => {
      const client = clientRef.current
      if (!client) {
        const err = new Error('Not connected')
        setError(err.message)
        throw err
      }
      setProvisioning(true)
      setError(null)
      try {
        setProvisioningStep('writingConfig')
        const sections: Record<string, JsonObject> = {
          mqtt: mqttSectionFromResolve(opts.mqtt),
        }
        const network = opts.networkSection ?? 'wifi'
        const wifiCapable = capabilities?.sections.some(
          (s) => s.name === network && s.configurable,
        )
        if (wifiCapable && opts.wifiSsid) {
          sections[network] = {
            ssid: opts.wifiSsid,
            password: opts.wifiPassword ?? '',
          }
        }
        const cellularCapable = capabilities?.sections.some(
          (s) => s.name === 'cellular' && s.configurable,
        )
        if (cellularCapable && opts.cellular && Object.keys(opts.cellular).length > 0) {
          sections.cellular = opts.cellular as JsonObject
        }
        const webhookCapable = capabilities?.sections.some(
          (s) => s.name === 'webhook' && s.configurable,
        )
        if (webhookCapable && opts.webhook && Object.keys(opts.webhook).length > 0) {
          sections.webhook = opts.webhook as JsonObject
        }

        // Unified gate: never send stub optional sections (empty APN / webhook
        // without URL / wifi without SSID) — devices reject those as
        // INVALID_ARGUMENT "<section>.<field>: required field missing".
        const pruned = pruneIncompleteConfigSections(sections)

        // Pass the long timeout into setConfig itself — the GATT transport's
        // per-request timer (default 15s) is what actually races the reply.
        const result = await withTimeout(
          client.setConfig(pruned, CONFIG_SET_TIMEOUT_MS),
          CONFIG_SET_TIMEOUT_MS + 5_000,
          'config.set',
        )
        setProvisioningStep('closing')
        try {
          await client.closeSession()
        } catch {
          // Device may drop link after close — acceptable
        }
        setProvisioningStep('done')
        setConnected(false)
        // Free GATT so the device is not left BUSY after a successful apply.
        clearBleRetain()
        await disposeSession()
        return result
      } catch (err: unknown) {
        const msg = err instanceof Error ? err.message : String(err)
        setError(msg)
        // Rethrow so the wizard can map ProvSessionError (e.g. wifi_init) to
        // operator copy instead of only reading a possibly-stale ct.error.
        throw err instanceof Error ? err : new Error(msg)
      } finally {
        setProvisioning(false)
        setProvisioningStep(null)
      }
    },
    [capabilities, clearBleRetain, disposeSession],
  )

  const disconnect = useCallback(() => {
    opGenRef.current += 1
    clearBleRetain()
    void disposeSession()
    setScanning(false)
    setConnecting(false)
    setProvisioning(false)
    setConnected(false)
    setDeviceInfo(null)
    setCapabilities(null)
    setSelectedScan(null)
    setError(null)
    setProvisioningStep(null)
    setScannedDevices([])
  }, [clearBleRetain, disposeSession])

  disconnectImplRef.current = disconnect

  return {
    channel,
    scanning,
    connecting,
    provisioning,
    connected,
    error,
    scannedDevices,
    deviceInfo,
    capabilities,
    selectedScan,
    provisioningStep,
    apModelHint,
    apHost,
    apSsidInput,
    checkAdapter,
    scan,
    selectBleDevice,
    connectAp,
    scanWifiNetworks,
    setApSsidInput,
    setApHost,
    setApModelHint,
    applyConfig,
    disconnect,
    clearError,
  }
}
