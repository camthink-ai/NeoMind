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
  openCtGattChannel,
  parseApSsid,
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
  /** Run config.set with wifi (+ mqtt from resolve) then session.close. */
  applyConfig: (opts: {
    wifiSsid?: string
    wifiPassword?: string
    mqtt: CtMqttConfig
    networkSection?: string
  }) => Promise<SetConfigResult | null>
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

  const clearError = useCallback(() => setError(null), [])

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
    setScanning(true)
    setError(null)
    setScannedDevices([])
    await new Promise((r) => setTimeout(r, 0))
    try {
      const devices = await scanCtDevicesWithWebStash({ durationMs: 10_000 })
      setScannedDevices(devices)
      return devices
    } catch (err: unknown) {
      if (err instanceof DOMException && err.name === 'NotFoundError') {
        setScannedDevices([])
        return []
      }
      setError(err instanceof Error ? err.message : String(err))
      return []
    } finally {
      setScanning(false)
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
      return true
    },
    [disposeSession],
  )

  const selectBleDevice = useCallback(
    async (device: CtScanDevice): Promise<boolean> => {
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
        const meta = await channelGatt.readMeta()
        if (meta?.busy) {
          channelGatt.disconnect()
          throw new Error('Device META reports BUSY')
        }
        const transport = new BleGattCtTransport(channelGatt)
        return await openWithTransport(transport)
      } catch (err: unknown) {
        setError(err instanceof Error ? err.message : String(err))
        setConnected(false)
        await disposeSession()
        return false
      } finally {
        setConnecting(false)
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
        setError('Not connected')
        return []
      }
      try {
        return await withTimeout(
          client.scanNetworks(section),
          25_000,
          'communication.scan',
        )
      } catch (err: unknown) {
        setError(err instanceof Error ? err.message : String(err))
        return []
      }
    },
    [],
  )

  const applyConfig = useCallback(
    async (opts: {
      wifiSsid?: string
      wifiPassword?: string
      mqtt: CtMqttConfig
      networkSection?: string
    }): Promise<SetConfigResult | null> => {
      const client = clientRef.current
      if (!client) {
        setError('Not connected')
        return null
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
        // Prefer active network if capabilities expose cellular-only (no wifi)
        if (!sections.wifi && !sections.halow) {
          const cellular = capabilities?.sections.find(
            (s) => s.name === 'cellular' && s.configurable,
          )
          if (cellular && capabilities?.currentNetwork === 'cellular') {
            // leave cellular as-is; mqtt-only write is valid
          }
        }

        const result = await withTimeout(client.setConfig(sections), 20_000, 'config.set')
        setProvisioningStep('closing')
        try {
          await client.closeSession()
        } catch {
          // Device may drop link after close — acceptable
        }
        setProvisioningStep('done')
        setConnected(false)
        return result
      } catch (err: unknown) {
        setError(err instanceof Error ? err.message : String(err))
        return null
      } finally {
        setProvisioning(false)
        setProvisioningStep(null)
      }
    },
    [capabilities],
  )

  const disconnect = useCallback(() => {
    void disposeSession()
    setConnected(false)
    setDeviceInfo(null)
    setCapabilities(null)
    setSelectedScan(null)
    setError(null)
    setProvisioningStep(null)
    setScannedDevices([])
  }, [disposeSession])

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
