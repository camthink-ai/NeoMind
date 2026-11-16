import { useState, useCallback, useEffect, useMemo } from 'react'
import { useTranslation } from 'react-i18next'
import {
  Wifi, Loader2, CheckCircle2, XCircle, Server, ChevronRight, Signal, Check,
} from 'lucide-react'

import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { PasswordInput } from '@/components/ui/password-input'
import { Label } from '@/components/ui/label'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { useCtProvision } from '@/hooks/useCtProvision'
import {
  listApProvisionModels,
  modelToDeviceType,
  parseApSsid,
  softapHostForModel,
  type CtMqttConfig,
} from '@/lib/ct-prov'
import { api, fetchAPI } from '@/lib/api'
import { useStore } from '@/store'

interface BrokerOption {
  id: string
  label: string
}

interface ApProvisionTabProps {
  onComplete?: (deviceId: string) => void
}

type Phase = 'guide' | 'connecting' | 'config' | 'provisioning' | 'done' | 'failed'

export function ApProvisionTab({ onComplete }: ApProvisionTabProps) {
  const { t } = useTranslation(['common', 'devices'])
  const ct = useCtProvision('ap')
  const deviceTypes = useStore((s) => s.deviceTypes)
  const apModels = useMemo(() => listApProvisionModels(deviceTypes), [deviceTypes])

  const [phase, setPhase] = useState<Phase>('guide')
  const [wifiSsid, setWifiSsid] = useState('')
  const [wifiPassword, setWifiPassword] = useState('')
  const [deviceName, setDeviceName] = useState('')
  const [brokerId, setBrokerId] = useState('embedded')
  const [brokers, setBrokers] = useState<BrokerOption[]>([])
  const [deviceId, setDeviceId] = useState<string | null>(null)
  const [registerError, setRegisterError] = useState<string | null>(null)
  const [isReprovision, setIsReprovision] = useState(false)
  const [modelPick, setModelPick] = useState<string>('NE101')

  useEffect(() => {
    if (apModels.length && !apModels.some((m) => m.model === modelPick)) {
      setModelPick(apModels[0]!.model)
      ct.setApHost(apModels[0]!.host)
    }
  }, [apModels]) // eslint-disable-line react-hooks/exhaustive-deps

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
      .catch(() => {})
  }, [t])

  useEffect(() => {
    const parsed = parseApSsid(ct.apSsidInput.trim())
    if (parsed?.modelHint) {
      const hint = parsed.modelHint
      const family =
        apModels.find((m) => hint === m.model || hint.startsWith(m.model))?.model ?? hint
      setModelPick(family)
      ct.setApHost(softapHostForModel(family, deviceTypes))
      ct.setApModelHint(hint)
    }
  }, [ct.apSsidInput, apModels, deviceTypes]) // eslint-disable-line react-hooks/exhaustive-deps

  const wifiSection = useMemo(() => {
    const sections = ct.capabilities?.sections ?? []
    if (sections.find((s) => s.name === 'wifi' && s.configurable)) return 'wifi'
    if (sections.find((s) => s.name === 'halow' && s.configurable)) return 'halow'
    return null
  }, [ct.capabilities])
  const needsWifi = wifiSection !== null

  const handleConnect = useCallback(async () => {
    setPhase('connecting')
    const host = softapHostForModel(modelPick, deviceTypes)
    ct.setApHost(host)
    const ok = await ct.connectAp({
      ssid: ct.apSsidInput || undefined,
      model: modelPick,
      host,
    })
    setPhase(ok ? 'config' : 'failed')
  }, [ct, modelPick, deviceTypes])

  const handleApply = useCallback(async () => {
    if (!ct.deviceInfo) return
    const modelName = ct.deviceInfo.model
    const deviceType = modelToDeviceType(modelName, deviceTypes)
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
      } catch {
        setDeviceId(resp.device_id)
      }
      setPhase('done')
    } catch (err) {
      setRegisterError(err instanceof Error ? err.message : String(err))
      setPhase('failed')
    }
  }, [ct, wifiSsid, wifiPassword, deviceName, brokerId, needsWifi, wifiSection, deviceTypes])

  const reset = useCallback(() => {
    ct.disconnect()
    setPhase('guide')
    setDeviceId(null)
    setRegisterError(null)
    setIsReprovision(false)
  }, [ct])

  if (phase === 'guide' && !ct.connected) {
    return (
      <div className="min-h-[480px] flex flex-col items-center justify-center gap-6 px-4">
        <div className="w-16 h-16 rounded-xl bg-primary-light flex items-center justify-center">
          <Wifi className="w-8 h-8 text-primary" />
        </div>
        <div className="text-center max-w-md">
          <h3 className="text-lg font-medium">{t('devices:ap.scanTitle')}</h3>
          <p className="text-sm text-muted-foreground mt-1">{t('devices:ap.scanDesc')}</p>
        </div>
        <div className="max-w-md w-full rounded-xl border bg-card p-4 space-y-2.5">
          <p className="text-xs font-medium text-muted-foreground uppercase tracking-wide">
            {t('devices:ap.prepTitle')}
          </p>
          <div className="flex items-start gap-2.5">
            <Check className="w-4 h-4 text-success mt-0.5 shrink-0" />
            <p className="text-sm">{t('devices:ap.prepStep1')}</p>
          </div>
          <div className="flex items-start gap-2.5">
            <Check className="w-4 h-4 text-success mt-0.5 shrink-0" />
            <p className="text-sm">{t('devices:ap.prepStep2')}</p>
          </div>
          <div className="flex items-start gap-2.5">
            <Check className="w-4 h-4 text-success mt-0.5 shrink-0" />
            <p className="text-sm text-muted-foreground">{t('devices:ap.prepStep3')}</p>
          </div>
          <p className="text-xs text-muted-foreground pt-1">{t('devices:ctProv.firmwareHint')}</p>
        </div>

        <div className="max-w-md w-full space-y-3">
          <div>
            <Label>{t('devices:ap.ssidOptional')}</Label>
            <Input
              value={ct.apSsidInput}
              onChange={(e) => ct.setApSsidInput(e.target.value)}
              placeholder={t('devices:ap.ssidPlaceholder')}
            />
          </div>
          <div>
            <Label>{t('devices:ap.model')}</Label>
            <Select
              value={modelPick}
              onValueChange={(v) => {
                setModelPick(v)
                ct.setApHost(softapHostForModel(v, deviceTypes))
                ct.setApModelHint(v)
              }}
            >
              <SelectTrigger>
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                {apModels.map((m) => (
                  <SelectItem key={m.model} value={m.model}>
                    {m.model} ({m.host})
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>
          <div>
            <Label>{t('devices:ap.host')}</Label>
            <Input
              value={ct.apHost}
              onChange={(e) => ct.setApHost(e.target.value)}
              placeholder="192.168.1.1"
            />
          </div>
          <Button className="w-full" onClick={handleConnect}>
            <Wifi className="w-4 h-4 mr-2" />
            {t('devices:ap.connectBtn')}
          </Button>
          {ct.error && <p className="text-sm text-error text-center">{ct.error}</p>}
        </div>
      </div>
    )
  }

  if (phase === 'connecting' || ct.connecting) {
    return (
      <div className="min-h-[480px] flex flex-col items-center justify-center gap-6 px-4">
        <div className="w-16 h-16 rounded-xl bg-primary-light flex items-center justify-center">
          <Loader2 className="w-8 h-8 text-primary animate-spin" />
        </div>
        <div className="text-center space-y-2">
          <h3 className="text-lg font-medium">{t('devices:ap.connectingTitle')}</h3>
          <p className="text-sm text-muted-foreground">{ct.apHost}</p>
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
          <p className="text-xs text-muted-foreground mt-2">{t('devices:ap.unreachableHint')}</p>
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
            <Wifi className="w-5 h-5 text-success" />
          </div>
          <div className="flex-1 min-w-0">
            <p className="text-sm font-medium truncate">
              {ct.deviceInfo?.deviceName || ct.deviceInfo?.model}
            </p>
            <p className="text-xs text-muted-foreground mt-0.5">
              <span className="text-success">{t('devices:ble.connected')}</span>
              <span className="ml-2">{ct.apHost}</span>
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
        />
      </div>

      {needsWifi && (
        <div className="rounded-xl border bg-card p-4 space-y-3">
          <div className="flex items-center gap-2">
            <Wifi className="w-4 h-4 text-primary" />
            <span className="text-sm font-medium">{t('devices:ble.wifiConfig')}</span>
          </div>
          <div>
            <Label>{t('devices:ble.ssid')}</Label>
            <Input value={wifiSsid} onChange={(e) => setWifiSsid(e.target.value)} />
          </div>
          <div>
            <Label>{t('devices:ble.password')}</Label>
            <PasswordInput value={wifiPassword} onChange={(e) => setWifiPassword(e.target.value)} />
          </div>
        </div>
      )}

      {!needsWifi && (
        <div className="rounded-xl border bg-card p-4 space-y-2">
          <div className="flex items-center gap-2">
            <Signal className="w-4 h-4 text-primary" />
            <span className="text-sm font-medium">{t('devices:ble.cat1NoWifi')}</span>
          </div>
        </div>
      )}

      <div className="rounded-xl border bg-card p-4 space-y-3">
        <div className="flex items-center gap-2">
          <Server className="w-4 h-4 text-primary" />
          <span className="text-sm font-medium">{t('devices:ble.mqttConfig')}</span>
        </div>
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

      <Button
        className="w-full"
        onClick={handleApply}
        disabled={(needsWifi && !wifiSsid) || ct.provisioning}
      >
        {ct.provisioning ? (
          <>
            <Loader2 className="w-4 h-4 mr-2 animate-spin" />
            {t('devices:ble.provisioning')}
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
