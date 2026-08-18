import { useState, useCallback, useEffect } from 'react'
import { useTranslation } from 'react-i18next'
import {
  Bluetooth, Wifi, Loader2, CheckCircle2, XCircle,
  Server, ChevronRight, Signal, Check,
} from 'lucide-react'

import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { PasswordInput } from '@/components/ui/password-input'
import { Label } from '@/components/ui/label'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { useBleProvision } from '@/hooks/useBleProvision'
import { parseBleDeviceName, modelToDeviceType, NETMOD_LABELS } from '@/lib/ble-protocol'
import type { BleMqttConfig } from '@/lib/ble-protocol'
import { api, fetchAPI } from '@/lib/api'
import { isTauriEnv } from '@/lib/ble-transport'
import type { BleTransportDevice } from '@/lib/ble-transport'

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

interface BrokerOption {
  id: string
  label: string
}

interface BleProvisionTabProps {
  onComplete?: (deviceId: string) => void
}

type Phase = 'scan' | 'connecting' | 'config' | 'provisioning' | 'done' | 'failed'

// ---------------------------------------------------------------------------
// Component
// ---------------------------------------------------------------------------

export function BleProvisionTab({ onComplete }: BleProvisionTabProps) {
  const { t } = useTranslation(['common', 'devices'])
  const ble = useBleProvision()

  const [phase, setPhase] = useState<Phase>('scan')
  const [wifiSsid, setWifiSsid] = useState('')
  const [wifiPassword, setWifiPassword] = useState('')
  const [deviceName, setDeviceName] = useState('')
  const [brokerId, setBrokerId] = useState('embedded')
  const [brokers, setBrokers] = useState<BrokerOption[]>([])
  const [deviceId, setDeviceId] = useState<string | null>(null)
  const [registerError, setRegisterError] = useState<string | null>(null)
  const [isReprovision, setIsReprovision] = useState(false)

  // Pre-fill SSID and device name from BLE READ
  useEffect(() => {
    if (ble.deviceConfig) {
      if (ble.deviceNetmod !== 'cat1' && ble.deviceConfig.ssid && !wifiSsid) setWifiSsid(ble.deviceConfig.ssid)
      if (ble.deviceConfig.device_name && !deviceName) setDeviceName(ble.deviceConfig.device_name)
    }
  }, [ble.deviceConfig])

  useEffect(() => {
    api.get<{ brokers: Array<{ id: string; name: string; broker: string }> }>('/brokers')
      .then((data) => {
        const list: BrokerOption[] = [{ id: 'embedded', label: t('devices:ble.embeddedBroker') }]
        for (const b of data.brokers) {
          list.push({ id: b.id, label: b.name || b.broker })
        }
        setBrokers(list)
      })
      .catch((err) => {
        console.warn('[BLE] Failed to load broker list:', err)
      })
  }, [t])

  // --- Scan ---
  const handleScan = useCallback(async () => {
    await ble.scan()
  }, [ble])

  // --- Select device → connect immediately ---
  const handleSelectDevice = useCallback(async (dev: BleTransportDevice) => {
    setPhase('connecting')
    const ok = await ble.selectDevice(dev)
    if (ok) {
      setPhase('config')
    } else {
      setPhase('failed')
    }
  }, [ble])

  // --- Apply: resolve MQTT → write config → register ---
  const handleApply = useCallback(async () => {
    if (!ble.device) return

    const modelName = ble.deviceModel || parseBleDeviceName(ble.device.name || '')?.model
    if (!modelName) {
      ble.disconnect()
      setPhase('failed')
      return
    }

    // Use device-provided SN or MAC (read from BLE characteristic), fallback to name
    const sn = ble.deviceSn || ble.deviceMac || ble.device.name || modelName

    const deviceType = modelToDeviceType(modelName)
    const name = deviceName || ble.device.name || modelName

    setPhase('provisioning')
    setRegisterError(null)

    try {
      // Phase 1: Resolve MQTT config
      const resp = await fetchAPI<{ device_id: string; mqtt_config: BleMqttConfig }>(
        '/devices/ble-provision', {
          method: 'POST',
          body: JSON.stringify({
            model: modelName,
            sn,
            device_type: deviceType,
            device_name: name,
            broker_id: brokerId,
            resolve_only: true,
          }),
        },
      )

      // Phase 2: Write config via existing BLE connection
      const ok = await ble.writeConfig(wifiSsid, wifiPassword, resp.mqtt_config, name)
      if (!ok) {
        setPhase('failed')
        return
      }

      // Phase 3: Register device (non-critical — BLE write already succeeded)
      try {
        const regResp = await fetchAPI<{ device_id: string; already_exists?: boolean }>(
          '/devices/ble-provision', {
            method: 'POST',
            body: JSON.stringify({
              model: modelName,
              sn,
              device_type: deviceType,
              device_name: name,
              broker_id: brokerId,
              resolve_only: false,
            }),
          },
        )
        setDeviceId(regResp.device_id)
        if (regResp.already_exists) setIsReprovision(true)
      } catch (regErr) {
        console.warn('[BLE] Device registration failed (BLE write succeeded):', regErr)
        setDeviceId(resp.device_id)
      }

      setPhase('done')
    } catch (err) {
      console.error('[BLE] Provisioning failed:', err)
      setRegisterError(err instanceof Error ? err.message : String(err))
      setPhase('failed')
    }
  }, [ble, wifiSsid, wifiPassword, deviceName, brokerId, onComplete])

  const reset = useCallback(() => {
    ble.disconnect()
    setPhase('scan')
    setDeviceId(null)
    setRegisterError(null)
    setIsReprovision(false)
  }, [ble])

  // ---------------------------------------------------------------------------
  // Render
  // ---------------------------------------------------------------------------

  const bleSupported = isTauriEnv() || (typeof navigator !== 'undefined' && !!navigator.bluetooth)

  const modelName = ble.deviceModel || parseBleDeviceName(ble.device?.name || '')?.model
  const macSuffix = ble.deviceMac || parseBleDeviceName(ble.device?.name || '')?.macSuffix

  // --- Scan phase ---
  if (phase === 'scan' && !ble.device) {
    // Scanning: show loading state
    if (ble.scanning) {
      return (
        <div className="min-h-[480px] flex flex-col items-center justify-center gap-6 px-4">
          <div className="w-16 h-16 rounded-xl bg-primary-light flex items-center justify-center">
            <Loader2 className="w-8 h-8 text-primary animate-spin" />
          </div>
          <div className="text-center space-y-2">
            <h3 className="text-lg font-medium">{t('devices:ble.scanning')}</h3>
            <p className="text-sm text-muted-foreground">{t('devices:ble.scanningHint')}</p>
          </div>
        </div>
      )
    }

    // Scan results: show device list
    if (ble.scannedDevices.length > 0) {
      return (
        <div className="min-h-[480px] flex flex-col px-4 py-6">
          <div className="text-center mb-6">
            <h3 className="text-lg font-medium">{t('devices:ble.selectDevice')}</h3>
            <p className="text-sm text-muted-foreground mt-1">
              {t('devices:ble.foundDevices', { count: ble.scannedDevices.length })}
            </p>
          </div>
          <div className="space-y-2 max-w-md mx-auto w-full">
            {ble.scannedDevices.map((dev) => {
              const parsed = dev.name ? parseBleDeviceName(dev.name) : null
              return (
                <button
                  key={dev.id}
                  onClick={() => handleSelectDevice(dev)}
                  className="w-full flex items-center gap-3 rounded-xl border bg-card p-3 hover:bg-muted-30 transition-colors text-left"
                >
                  <div className="w-10 h-10 rounded-lg bg-primary-light flex items-center justify-center shrink-0">
                    <Bluetooth className="w-5 h-5 text-primary" />
                  </div>
                  <div className="flex-1 min-w-0">
                    <p className="text-sm font-medium truncate">{parsed ? parsed.model : dev.name}</p>
                    {parsed && (
                      <p className="text-xs text-muted-foreground">
                        {parsed.mac}
                      </p>
                    )}
                  </div>
                  {dev.rssi != null && (
                    <div className="flex items-center gap-1 text-xs text-muted-foreground shrink-0">
                      <Signal className="w-3 h-3" />
                      {dev.rssi} dBm
                    </div>
                  )}
                </button>
              )
            })}
          </div>
          <div className="flex justify-center mt-6">
            <Button variant="outline" onClick={handleScan} disabled={ble.scanning}>
              <Bluetooth className="w-4 h-4 mr-2" />
              {t('devices:ble.rescan')}
            </Button>
          </div>
        </div>
      )
    }

    // Idle: show scan button
    return (
      <div className="min-h-[480px] flex flex-col items-center justify-center gap-6 px-4">
        <div className="w-16 h-16 rounded-xl bg-primary-light flex items-center justify-center">
          <Bluetooth className="w-8 h-8 text-primary" />
        </div>
        <div className="text-center">
          <h3 className="text-lg font-medium">{t('devices:ble.scanTitle')}</h3>
          <p className="text-sm text-muted-foreground mt-1">{t('devices:ble.scanDesc')}</p>
        </div>
        {/* Preparation guide */}
        <div className="max-w-sm w-full rounded-xl border bg-card p-4 space-y-2.5">
          <p className="text-xs font-medium text-muted-foreground uppercase tracking-wide">{t('devices:ble.prepTitle')}</p>
          <div className="flex items-start gap-2.5">
            <Check className="w-4 h-4 text-success mt-0.5 shrink-0" />
            <p className="text-sm">{t('devices:ble.prepStep1')}</p>
          </div>
          <div className="flex items-start gap-2.5">
            <Check className="w-4 h-4 text-success mt-0.5 shrink-0" />
            <p className="text-sm">{t('devices:ble.prepStep2')}</p>
          </div>
          <div className="flex items-start gap-2.5">
            <Check className="w-4 h-4 text-success mt-0.5 shrink-0" />
            <p className="text-sm text-muted-foreground">{t('devices:ble.prepStep3')}</p>
          </div>
        </div>
        {!bleSupported ? (
          <div className="text-center space-y-3 max-w-sm">
            <p className="text-sm text-warning">{t('devices:ble.notSupported')}</p>
            <p className="text-xs text-muted-foreground">{t('devices:ble.notSupportedHint')}</p>
          </div>
        ) : (
          <Button onClick={handleScan}>
            <Bluetooth className="w-4 h-4 mr-2" />
            {t('devices:ble.scanBtn')}
          </Button>
        )}
        {ble.error && <p className="text-sm text-error text-center max-w-sm">{ble.error}</p>}
      </div>
    )
  }

  // --- Connecting phase (selecting device) ---
  if (phase === 'connecting' || (ble.connecting && phase !== 'provisioning')) {
    return (
      <div className="min-h-[480px] flex flex-col items-center justify-center gap-6 px-4">
        <div className="w-16 h-16 rounded-xl bg-primary-light flex items-center justify-center">
          <Loader2 className="w-8 h-8 text-primary animate-spin" />
        </div>
        <div className="text-center space-y-2">
          <h3 className="text-lg font-medium">{t('devices:ble.connectingTitle')}</h3>
          <p className="text-sm text-muted-foreground">{ble.device?.name}</p>
        </div>
      </div>
    )
  }

  // --- Done ---
  if (phase === 'done') {
    return (
      <div className="min-h-[480px] flex flex-col items-center justify-center gap-4 px-4">
        <div className="w-16 h-16 rounded-xl bg-success-light flex items-center justify-center">
          <CheckCircle2 className="w-8 h-8 text-success" />
        </div>
        <div className="text-center">
          <h3 className="text-lg font-medium">
            {isReprovision ? t('devices:ble.reprovisionTitle') : t('devices:ble.successTitle')}
          </h3>
          <p className="text-sm text-muted-foreground mt-1">
            {isReprovision ? t('devices:ble.reprovisionDesc') : t('devices:ble.successDesc')}
          </p>
        </div>
        {deviceId && !isReprovision && <p className="text-sm text-success">{t('devices:ble.registered', { id: deviceId })}</p>}
        <Button variant="outline" onClick={() => { if (deviceId) onComplete?.(deviceId) }}>
          {t('common:close')}
        </Button>
      </div>
    )
  }

  // --- Failed ---
  if (phase === 'failed') {
    return (
      <div className="min-h-[480px] flex flex-col items-center justify-center gap-6 px-4">
        <div className="w-16 h-16 rounded-xl bg-error-light flex items-center justify-center">
          <XCircle className="w-8 h-8 text-error" />
        </div>
        <div className="text-center">
          <h3 className="text-lg font-medium">{t('devices:ble.failedTitle')}</h3>
          <p className="text-sm text-muted-foreground mt-1">{ble.error || registerError}</p>
        </div>
        <Button onClick={reset}>{t('common:retry')}</Button>
      </div>
    )
  }

  // --- Config or provisioning ---
  return (
    <div className="max-w-md mx-auto py-4 space-y-4">
      {/* Device info header — show connection status */}
      <div className="rounded-xl border bg-card p-4">
        <div className="flex items-center gap-3">
          <div className="w-10 h-10 rounded-lg bg-success-light flex items-center justify-center shrink-0">
            <Bluetooth className="w-5 h-5 text-success" />
          </div>
          <div className="flex-1 min-w-0">
            <p className="text-sm font-medium truncate">
              {ble.deviceModel || parseBleDeviceName(ble.device?.name || '')?.model || ble.device?.name}
            </p>
            <p className="text-xs text-muted-foreground mt-0.5">
              <span className="text-success">{t('devices:ble.connected')}</span>
              {ble.deviceSn && <span className="ml-2">SN: {ble.deviceSn}</span>}
              {!ble.deviceSn && ble.deviceMac && <span className="ml-2">MAC: {ble.deviceMac}</span>}
              {ble.deviceNetmod && (
                <span className="ml-2">{NETMOD_LABELS[ble.deviceNetmod]}</span>
              )}
            </p>
          </div>
        </div>
      </div>

      {/* Device name */}
      <div className="rounded-xl border bg-card p-4 space-y-2">
        <Label className="text-xs text-muted-foreground">{t('devices:ble.deviceName')}</Label>
        <Input
          value={deviceName}
          onChange={(e) => setDeviceName(e.target.value)}
          placeholder={modelName || 'Camera'}
          className="text-base"
        />
      </div>

      {/* WiFi config — hidden for Cat.1 devices */}
      {(!ble.deviceNetmod || ble.deviceNetmod !== 'cat1') && (
        <div className="rounded-xl border bg-card p-4 space-y-3">
          <div className="flex items-center gap-2">
            <Wifi className="w-4 h-4 text-primary" />
            <span className="text-sm font-medium">
              {ble.deviceNetmod === 'halow'
                ? t('devices:ble.wifiConfigHalow')
                : t('devices:ble.wifiConfig')}
            </span>
          </div>
          <div>
            <Label>{t('devices:ble.ssid')}</Label>
            <Input value={wifiSsid} onChange={(e) => setWifiSsid(e.target.value)} placeholder={t('devices:ble.ssidPlaceholder')} />
          </div>
          <div>
            <Label>{t('devices:ble.password')}</Label>
            <PasswordInput value={wifiPassword} onChange={(e) => setWifiPassword(e.target.value)} placeholder={t('devices:ble.passwordPlaceholder')} />
          </div>
        </div>
      )}

      {/* Cat.1 info notice */}
      {ble.deviceNetmod === 'cat1' && (
        <div className="rounded-xl border bg-card p-4 space-y-2">
          <div className="flex items-center gap-2">
            <Signal className="w-4 h-4 text-primary" />
            <span className="text-sm font-medium">{t('devices:ble.cat1NoWifi')}</span>
          </div>
          <p className="text-xs text-muted-foreground">{t('devices:ble.cat1Hint')}</p>
        </div>
      )}

      {/* MQTT broker */}
      <div className="rounded-xl border bg-card p-4 space-y-3">
        <div className="flex items-center gap-2">
          <Server className="w-4 h-4 text-primary" />
          <span className="text-sm font-medium">{t('devices:ble.mqttConfig')}</span>
        </div>
        <div>
          <Label>{t('devices:ble.broker')}</Label>
          <Select value={brokerId} onValueChange={setBrokerId}>
            <SelectTrigger><SelectValue /></SelectTrigger>
            <SelectContent>
              {brokers.map((b) => (
                <SelectItem key={b.id} value={b.id}>{b.label}</SelectItem>
              ))}
            </SelectContent>
          </Select>
        </div>
      </div>

      {/* Apply button */}
      <Button
        className="w-full"
        onClick={handleApply}
        disabled={(ble.deviceNetmod !== 'cat1' && !wifiSsid) || ble.provisioning}
      >
        {ble.provisioning ? (
          <>
            <Loader2 className="w-4 h-4 mr-2 animate-spin" />
            {ble.provisioningStep === 'writingMqtt' && t('devices:ble.status.writingMqtt')}
            {ble.provisioningStep === 'done' && t('devices:ble.status.done')}
            {(!ble.provisioningStep || (ble.provisioningStep !== 'writingMqtt' && ble.provisioningStep !== 'done')) && t('devices:ble.provisioning')}
          </>
        ) : (
          <>
            <ChevronRight className="w-4 h-4 mr-2" />
            {t('devices:ble.apply')}
          </>
        )}
      </Button>

      {ble.error && (
        <div className="text-center text-sm text-error">
          {ble.error}
        </div>
      )}
    </div>
  )
}
