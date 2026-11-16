import { useState, useCallback, useEffect, useMemo } from 'react'
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
import { useCtProvision } from '@/hooks/useCtProvision'
import { isTauriEnv, modelToDeviceType, type CtMqttConfig, type CtScanDevice } from '@/lib/ct-prov'
import { api, fetchAPI } from '@/lib/api'

interface BrokerOption {
  id: string
  label: string
}

interface BleProvisionTabProps {
  onComplete?: (deviceId: string) => void
}

type Phase = 'scan' | 'connecting' | 'config' | 'provisioning' | 'done' | 'failed'

export function BleProvisionTab({ onComplete }: BleProvisionTabProps) {
  const { t } = useTranslation(['common', 'devices'])
  const ct = useCtProvision('ble')

  const [phase, setPhase] = useState<Phase>('scan')
  const [wifiSsid, setWifiSsid] = useState('')
  const [wifiPassword, setWifiPassword] = useState('')
  const [deviceName, setDeviceName] = useState('')
  const [brokerId, setBrokerId] = useState('embedded')
  const [brokers, setBrokers] = useState<BrokerOption[]>([])
  const [deviceId, setDeviceId] = useState<string | null>(null)
  const [registerError, setRegisterError] = useState<string | null>(null)
  const [isReprovision, setIsReprovision] = useState(false)

  useEffect(() => {
    if (ct.deviceInfo?.deviceName && !deviceName) {
      setDeviceName(ct.deviceInfo.deviceName)
    }
  }, [ct.deviceInfo, deviceName])

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
        console.warn('[CT-Prov] Failed to load broker list:', err)
      })
  }, [t])

  const wifiSection = useMemo(() => {
    const sections = ct.capabilities?.sections ?? []
    const wifi = sections.find((s) => s.name === 'wifi' && s.configurable)
    if (wifi) return 'wifi'
    const halow = sections.find((s) => s.name === 'halow' && s.configurable)
    if (halow) return 'halow'
    return null
  }, [ct.capabilities])

  const needsWifi = wifiSection !== null

  const handleScan = useCallback(async () => {
    await ct.scan()
  }, [ct])

  const handleSelectDevice = useCallback(async (dev: CtScanDevice) => {
    setPhase('connecting')
    const ok = await ct.selectBleDevice(dev)
    setPhase(ok ? 'config' : 'failed')
  }, [ct])

  const handleApply = useCallback(async () => {
    if (!ct.deviceInfo) return
    const modelName = ct.deviceInfo.model
    const deviceType = modelToDeviceType(modelName)
    if (!deviceType) {
      setRegisterError(`Unknown model: ${modelName}`)
      setPhase('failed')
      return
    }
    const sn = ct.deviceInfo.serialNumber
    const name = deviceName || ct.deviceInfo.deviceName || modelName

    setPhase('provisioning')
    setRegisterError(null)

    try {
      const resp = await fetchAPI<{ device_id: string; mqtt_config: CtMqttConfig }>(
        '/devices/ble-provision',
        {
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

      const setResult = await ct.applyConfig({
        wifiSsid: needsWifi ? wifiSsid : undefined,
        wifiPassword: needsWifi ? wifiPassword : undefined,
        mqtt: resp.mqtt_config,
        networkSection: wifiSection ?? undefined,
      })
      if (!setResult) {
        setPhase('failed')
        return
      }

      try {
        const regResp = await fetchAPI<{ device_id: string; already_exists?: boolean }>(
          '/devices/ble-provision',
          {
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
        console.warn('[CT-Prov] Device registration failed (config.set succeeded):', regErr)
        setDeviceId(resp.device_id)
      }

      setPhase('done')
    } catch (err) {
      console.error('[CT-Prov] Provisioning failed:', err)
      setRegisterError(err instanceof Error ? err.message : String(err))
      setPhase('failed')
    }
  }, [ct, wifiSsid, wifiPassword, deviceName, brokerId, needsWifi, wifiSection])

  const reset = useCallback(() => {
    ct.disconnect()
    setPhase('scan')
    setDeviceId(null)
    setRegisterError(null)
    setIsReprovision(false)
  }, [ct])

  const bleSupported = isTauriEnv() || (typeof navigator !== 'undefined' && !!navigator.bluetooth)

  if (phase === 'scan' && !ct.connected && !ct.deviceInfo) {
    if (ct.scanning) {
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

    if (ct.scannedDevices.length > 0) {
      return (
        <div className="min-h-[480px] flex flex-col px-4 py-6">
          <div className="text-center mb-6">
            <h3 className="text-lg font-medium">{t('devices:ble.selectDevice')}</h3>
            <p className="text-sm text-muted-foreground mt-1">
              {t('devices:ble.foundDevices', { count: ct.scannedDevices.length })}
            </p>
          </div>
          <div className="space-y-2 max-w-md mx-auto w-full">
            {ct.scannedDevices.map((dev) => {
              const busy = dev.discovered?.busy
              const label =
                dev.discovered?.displayName ||
                dev.name ||
                dev.discovered?.modelCodeName ||
                dev.id
              return (
                <button
                  key={dev.id}
                  onClick={() => !busy && handleSelectDevice(dev)}
                  disabled={!!busy}
                  className="w-full flex items-center gap-3 rounded-xl border bg-card p-3 hover:bg-muted-30 transition-colors text-left disabled:opacity-50"
                >
                  <div className="w-10 h-10 rounded-lg bg-primary-light flex items-center justify-center shrink-0">
                    <Bluetooth className="w-5 h-5 text-primary" />
                  </div>
                  <div className="flex-1 min-w-0">
                    <p className="text-sm font-medium truncate">{label}</p>
                    <p className="text-xs text-muted-foreground">
                      {busy ? t('devices:ble.busy') : t('devices:ctProv.productName')}
                      {dev.discovered?.macSuffix ? ` · ${dev.discovered.macSuffix}` : ''}
                    </p>
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
            <Button variant="outline" onClick={handleScan} disabled={ct.scanning}>
              <Bluetooth className="w-4 h-4 mr-2" />
              {t('devices:ble.rescan')}
            </Button>
          </div>
        </div>
      )
    }

    return (
      <div className="min-h-[480px] flex flex-col items-center justify-center gap-6 px-4">
        <div className="w-16 h-16 rounded-xl bg-primary-light flex items-center justify-center">
          <Bluetooth className="w-8 h-8 text-primary" />
        </div>
        <div className="text-center">
          <h3 className="text-lg font-medium">{t('devices:ble.scanTitle')}</h3>
          <p className="text-sm text-muted-foreground mt-1">{t('devices:ble.scanDesc')}</p>
        </div>
        <div className="max-w-sm w-full rounded-xl border bg-card p-4 space-y-2.5">
          <p className="text-xs font-medium text-muted-foreground uppercase tracking-wide">
            {t('devices:ble.prepTitle')}
          </p>
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
          <p className="text-xs text-muted-foreground pt-1">{t('devices:ctProv.firmwareHint')}</p>
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
        {ct.error && <p className="text-sm text-error text-center max-w-sm">{ct.error}</p>}
      </div>
    )
  }

  if (phase === 'connecting' || (ct.connecting && phase !== 'provisioning')) {
    return (
      <div className="min-h-[480px] flex flex-col items-center justify-center gap-6 px-4">
        <div className="w-16 h-16 rounded-xl bg-primary-light flex items-center justify-center">
          <Loader2 className="w-8 h-8 text-primary animate-spin" />
        </div>
        <div className="text-center space-y-2">
          <h3 className="text-lg font-medium">{t('devices:ble.connectingTitle')}</h3>
          <p className="text-sm text-muted-foreground">
            {ct.selectedScan?.name || ct.selectedScan?.id}
          </p>
        </div>
      </div>
    )
  }

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
        {deviceId && !isReprovision && (
          <p className="text-sm text-success">{t('devices:ble.registered', { id: deviceId })}</p>
        )}
        <Button
          variant="outline"
          onClick={() => {
            if (deviceId) onComplete?.(deviceId)
          }}
        >
          {t('common:close')}
        </Button>
      </div>
    )
  }

  if (phase === 'failed') {
    return (
      <div className="min-h-[480px] flex flex-col items-center justify-center gap-6 px-4">
        <div className="w-16 h-16 rounded-xl bg-error-light flex items-center justify-center">
          <XCircle className="w-8 h-8 text-error" />
        </div>
        <div className="text-center">
          <h3 className="text-lg font-medium">{t('devices:ble.failedTitle')}</h3>
          <p className="text-sm text-muted-foreground mt-1">{ct.error || registerError}</p>
        </div>
        <Button onClick={reset}>{t('common:retry')}</Button>
      </div>
    )
  }

  return (
    <div className="max-w-md mx-auto py-4 space-y-4">
      <div className="rounded-xl border bg-card p-4">
        <div className="flex items-center gap-3">
          <div className="w-10 h-10 rounded-lg bg-success-light flex items-center justify-center shrink-0">
            <Bluetooth className="w-5 h-5 text-success" />
          </div>
          <div className="flex-1 min-w-0">
            <p className="text-sm font-medium truncate">
              {ct.deviceInfo?.deviceName || ct.deviceInfo?.model}
            </p>
            <p className="text-xs text-muted-foreground mt-0.5">
              <span className="text-success">{t('devices:ble.connected')}</span>
              {ct.deviceInfo?.serialNumber && (
                <span className="ml-2">SN: {ct.deviceInfo.serialNumber}</span>
              )}
              {ct.deviceInfo?.model && <span className="ml-2">{ct.deviceInfo.model}</span>}
            </p>
          </div>
        </div>
      </div>

      <div className="rounded-xl border bg-card p-4 space-y-2">
        <Label className="text-xs text-muted-foreground">{t('devices:ble.deviceName')}</Label>
        <Input
          value={deviceName}
          onChange={(e) => setDeviceName(e.target.value)}
          placeholder={ct.deviceInfo?.model || 'Camera'}
          className="text-base"
        />
      </div>

      {needsWifi && (
        <div className="rounded-xl border bg-card p-4 space-y-3">
          <div className="flex items-center gap-2">
            <Wifi className="w-4 h-4 text-primary" />
            <span className="text-sm font-medium">
              {wifiSection === 'halow' ? t('devices:ble.wifiConfigHalow') : t('devices:ble.wifiConfig')}
            </span>
          </div>
          <div>
            <Label>{t('devices:ble.ssid')}</Label>
            <Input
              value={wifiSsid}
              onChange={(e) => setWifiSsid(e.target.value)}
              placeholder={t('devices:ble.ssidPlaceholder')}
            />
          </div>
          <div>
            <Label>{t('devices:ble.password')}</Label>
            <PasswordInput
              value={wifiPassword}
              onChange={(e) => setWifiPassword(e.target.value)}
              placeholder={t('devices:ble.passwordPlaceholder')}
            />
          </div>
        </div>
      )}

      {!needsWifi && (
        <div className="rounded-xl border bg-card p-4 space-y-2">
          <div className="flex items-center gap-2">
            <Signal className="w-4 h-4 text-primary" />
            <span className="text-sm font-medium">{t('devices:ble.cat1NoWifi')}</span>
          </div>
          <p className="text-xs text-muted-foreground">{t('devices:ble.cat1Hint')}</p>
        </div>
      )}

      <div className="rounded-xl border bg-card p-4 space-y-3">
        <div className="flex items-center gap-2">
          <Server className="w-4 h-4 text-primary" />
          <span className="text-sm font-medium">{t('devices:ble.mqttConfig')}</span>
        </div>
        <div>
          <Label>{t('devices:ble.broker')}</Label>
          <Select value={brokerId} onValueChange={setBrokerId}>
            <SelectTrigger>
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {brokers.map((b) => (
                <SelectItem key={b.id} value={b.id}>
                  {b.label}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </div>
      </div>

      <Button
        className="w-full"
        onClick={handleApply}
        disabled={(needsWifi && !wifiSsid) || ct.provisioning}
      >
        {ct.provisioning ? (
          <>
            <Loader2 className="w-4 h-4 mr-2 animate-spin" />
            {ct.provisioningStep === 'writingConfig' && t('devices:ble.status.writingMqtt')}
            {ct.provisioningStep === 'closing' && t('devices:ctProv.closingSession')}
            {ct.provisioningStep === 'done' && t('devices:ble.status.done')}
            {!ct.provisioningStep && t('devices:ble.provisioning')}
          </>
        ) : (
          <>
            <ChevronRight className="w-4 h-4 mr-2" />
            {t('devices:ble.apply')}
          </>
        )}
      </Button>

      {ct.error && <div className="text-center text-sm text-error">{ct.error}</div>}
    </div>
  )
}
