/**
 * Shared CamThink CT-Prov post-connect wizard.
 * Steps: info → capabilities → edit → applying (config.set + register) → done | failed.
 * BLE and SoftAP tabs own discover/join; this composition starts only after session open.
 */

import { useCallback, useEffect, useMemo, useState, type ReactNode } from 'react'
import { useTranslation } from 'react-i18next'
import {
  Bluetooth,
  CheckCircle2,
  ChevronDown,
  ChevronRight,
  Loader2,
  Server,
  Signal,
  Wifi,
  XCircle,
} from 'lucide-react'

import { Button } from '@/components/ui/button'
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from '@/components/ui/collapsible'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { PasswordInput } from '@/components/ui/password-input'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import type { UseCtProvisionReturn } from '@/hooks/useCtProvision'
import { modelToDeviceType, type CtMqttConfig } from '@/lib/ct-prov'
import { api, fetchAPI } from '@/lib/api'
import { useStore } from '@/store'
import { cn } from '@/lib/utils'

export type CtWizardStep = 'info' | 'capabilities' | 'edit' | 'applying' | 'done' | 'failed'

interface BrokerOption {
  id: string
  label: string
}

export interface CtProvisionWizardProps {
  channel: 'ble' | 'ap'
  ct: UseCtProvisionReturn
  onComplete?: (deviceId: string) => void
  /** Called when user abandons / retries back to discover or join. */
  onReset: () => void
  /** Extra failure hint (SoftAP unreachable etc.). */
  failureHint?: string
}

const STEP_ORDER: CtWizardStep[] = ['info', 'capabilities', 'edit']

function StepDots({ current }: { current: CtWizardStep }) {
  const { t } = useTranslation('devices')
  const idx = STEP_ORDER.indexOf(current)
  const labels = [
    t('ctProv.stepInfo'),
    t('ctProv.stepCapabilities'),
    t('ctProv.stepConfigure'),
  ]
  return (
    <div className="flex items-center justify-center gap-2 mb-6" aria-hidden={idx < 0}>
      {STEP_ORDER.map((step, i) => {
        const active = i === idx
        const done = idx > i
        return (
          <div key={step} className="flex items-center gap-2">
            {i > 0 && <div className={cn('h-px w-4', done || active ? 'bg-primary' : 'bg-border')} />}
            <div className="flex flex-col items-center gap-1">
              <div
                className={cn(
                  'w-6 h-6 rounded-lg text-xs font-medium flex items-center justify-center',
                  done && 'bg-success-light text-success',
                  active && 'bg-primary text-primary-foreground',
                  !done && !active && 'bg-muted text-muted-foreground',
                )}
              >
                {i + 1}
              </div>
              <span
                className={cn(
                  'text-[10px] hidden sm:block',
                  active ? 'text-foreground' : 'text-muted-foreground',
                )}
              >
                {labels[i]}
              </span>
            </div>
          </div>
        )
      })}
    </div>
  )
}

export function CtProvisionCenter({
  children,
  className,
}: {
  children: ReactNode
  className?: string
}) {
  return (
    <div
      className={cn(
        'min-h-[420px] sm:min-h-[480px] flex flex-col items-center justify-center gap-6 px-4 py-6',
        className,
      )}
    >
      {children}
    </div>
  )
}

export function CtProvisionWizard({
  channel,
  ct,
  onComplete,
  onReset,
  failureHint,
}: CtProvisionWizardProps) {
  const { t } = useTranslation(['common', 'devices'])
  const deviceTypes = useStore((s) => s.deviceTypes)
  const ChannelIcon = channel === 'ble' ? Bluetooth : Wifi

  const [step, setStep] = useState<CtWizardStep>('info')
  const [wifiSsid, setWifiSsid] = useState('')
  const [wifiPassword, setWifiPassword] = useState('')
  const [deviceName, setDeviceName] = useState('')
  const [brokerId, setBrokerId] = useState('embedded')
  const [brokers, setBrokers] = useState<BrokerOption[]>([])
  const [deviceId, setDeviceId] = useState<string | null>(null)
  const [registerError, setRegisterError] = useState<string | null>(null)
  const [isReprovision, setIsReprovision] = useState(false)
  const [advancedOpen, setAdvancedOpen] = useState(false)
  const [capsDetailOpen, setCapsDetailOpen] = useState(false)

  useEffect(() => {
    if (ct.deviceInfo?.deviceName && !deviceName) {
      setDeviceName(ct.deviceInfo.deviceName)
    }
  }, [ct.deviceInfo, deviceName])

  useEffect(() => {
    api
      .get<{ brokers: Array<{ id: string; name: string; broker: string }> }>('/brokers')
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
    if (sections.find((s) => s.name === 'wifi' && s.configurable)) return 'wifi'
    if (sections.find((s) => s.name === 'halow' && s.configurable)) return 'halow'
    return null
  }, [ct.capabilities])
  const needsWifi = wifiSection !== null

  const handleApply = useCallback(async () => {
    if (!ct.deviceInfo) return
    const modelName = ct.deviceInfo.model
    const deviceType = modelToDeviceType(modelName, deviceTypes)
    if (!deviceType) {
      setRegisterError(`Unknown model: ${modelName}`)
      setStep('failed')
      return
    }
    const sn = ct.deviceInfo.serialNumber
    const name = deviceName || ct.deviceInfo.deviceName || modelName

    setStep('applying')
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
        setStep('failed')
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

      setStep('done')
    } catch (err) {
      console.error('[CT-Prov] Provisioning failed:', err)
      setRegisterError(err instanceof Error ? err.message : String(err))
      setStep('failed')
    }
  }, [ct, wifiSsid, wifiPassword, deviceName, brokerId, needsWifi, wifiSection, deviceTypes])

  if (step === 'applying') {
    return (
      <CtProvisionCenter>
        <div className="w-16 h-16 rounded-xl bg-primary-light flex items-center justify-center">
          <Loader2 className="w-8 h-8 text-primary animate-spin" />
        </div>
        <div className="text-center space-y-2">
          <h3 className="text-lg font-medium">{t('devices:ble.provisioning')}</h3>
          <p className="text-sm text-muted-foreground">
            {ct.provisioningStep === 'writingConfig' && t('devices:ble.status.writingMqtt')}
            {ct.provisioningStep === 'closing' && t('devices:ctProv.closingSession')}
            {ct.provisioningStep === 'done' && t('devices:ble.status.done')}
            {!ct.provisioningStep && t('devices:ctProv.applyingHint')}
          </p>
        </div>
      </CtProvisionCenter>
    )
  }

  if (step === 'done') {
    return (
      <CtProvisionCenter>
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
      </CtProvisionCenter>
    )
  }

  if (step === 'failed') {
    return (
      <CtProvisionCenter>
        <div className="w-16 h-16 rounded-xl bg-error-light flex items-center justify-center">
          <XCircle className="w-8 h-8 text-error" />
        </div>
        <div className="text-center max-w-sm">
          <h3 className="text-lg font-medium">{t('devices:ble.failedTitle')}</h3>
          <p className="text-sm text-muted-foreground mt-1">{ct.error || registerError}</p>
          {failureHint && (
            <p className="text-xs text-muted-foreground mt-2">{failureHint}</p>
          )}
        </div>
        <Button onClick={onReset}>{t('common:retry')}</Button>
      </CtProvisionCenter>
    )
  }

  return (
    <div className="max-w-md mx-auto py-4 px-1 sm:px-0">
      <StepDots current={step} />

      {/* Connected strip */}
      <div className="rounded-xl border bg-card p-3 mb-4">
        <div className="flex items-center gap-3">
          <div className="w-9 h-9 rounded-lg bg-success-light flex items-center justify-center shrink-0">
            <ChannelIcon className="w-4 h-4 text-success" />
          </div>
          <div className="flex-1 min-w-0">
            <p className="text-sm font-medium truncate">
              {ct.deviceInfo?.deviceName || ct.deviceInfo?.model}
            </p>
            <p className="text-xs text-muted-foreground">
              <span className="text-success">{t('devices:ble.connected')}</span>
              {ct.deviceInfo?.model && <span className="ml-2">{ct.deviceInfo.model}</span>}
            </p>
          </div>
        </div>
      </div>

      {step === 'info' && (
        <div className="space-y-4">
          <div className="text-center mb-2">
            <h3 className="text-base font-medium">{t('devices:ctProv.stepInfoTitle')}</h3>
            <p className="text-sm text-muted-foreground mt-1">{t('devices:ctProv.stepInfoDesc')}</p>
          </div>
          <div className="rounded-xl border bg-card p-4 space-y-3 text-sm">
            <InfoRow label={t('devices:ble.deviceName')} value={ct.deviceInfo?.deviceName || '—'} />
            <InfoRow label={t('devices:ctProv.serialNumber')} value={ct.deviceInfo?.serialNumber || '—'} />
            <InfoRow label={t('devices:ctProv.model')} value={ct.deviceInfo?.model || '—'} />
            <InfoRow label={t('devices:ctProv.firmware')} value={ct.deviceInfo?.firmware || '—'} />
          </div>
          <Button className="w-full" onClick={() => setStep('capabilities')}>
            <ChevronRight className="w-4 h-4 mr-2" />
            {t('devices:ctProv.continue')}
          </Button>
        </div>
      )}

      {step === 'capabilities' && (
        <div className="space-y-4">
          <div className="text-center mb-2">
            <h3 className="text-base font-medium">{t('devices:ctProv.stepCapsTitle')}</h3>
            <p className="text-sm text-muted-foreground mt-1">{t('devices:ctProv.stepCapsDesc')}</p>
          </div>
          <div className="rounded-xl border bg-card p-4 space-y-2">
            <p className="text-sm">
              {needsWifi
                ? wifiSection === 'halow'
                  ? t('devices:ble.wifiConfigHalow')
                  : t('devices:ble.wifiConfig')
                : t('devices:ble.cat1NoWifi')}
            </p>
            {ct.capabilities?.currentNetwork && (
              <p className="text-xs text-muted-foreground">
                {t('devices:ctProv.currentNetwork', { network: ct.capabilities.currentNetwork })}
              </p>
            )}
            <Collapsible open={capsDetailOpen} onOpenChange={setCapsDetailOpen}>
              <CollapsibleTrigger className="flex items-center gap-1 text-xs text-muted-foreground hover:text-foreground pt-1">
                <ChevronDown
                  className={cn('w-3.5 h-3.5 transition-transform', capsDetailOpen && 'rotate-180')}
                />
                {t('devices:ctProv.showCapabilities')}
              </CollapsibleTrigger>
              <CollapsibleContent className="pt-2 space-y-1">
                {(ct.capabilities?.sections ?? []).map((s) => (
                  <div
                    key={s.name}
                    className="flex items-center justify-between text-xs rounded-lg bg-muted-30 px-2.5 py-1.5"
                  >
                    <span className="font-medium">{s.name}</span>
                    <span className="text-muted-foreground">
                      {s.kind}
                      {s.configurable ? ` · ${t('devices:ctProv.configurable')}` : ''}
                    </span>
                  </div>
                ))}
                {!ct.capabilities?.sections?.length && (
                  <p className="text-xs text-muted-foreground">{t('devices:ctProv.noSections')}</p>
                )}
              </CollapsibleContent>
            </Collapsible>
          </div>
          <div className="flex gap-2">
            <Button variant="outline" className="flex-1" onClick={() => setStep('info')}>
              {t('common:back')}
            </Button>
            <Button className="flex-1" onClick={() => setStep('edit')}>
              <ChevronRight className="w-4 h-4 mr-2" />
              {t('devices:ctProv.continue')}
            </Button>
          </div>
        </div>
      )}

      {step === 'edit' && (
        <div className="space-y-4">
          <div className="text-center mb-2">
            <h3 className="text-base font-medium">{t('devices:ctProv.stepEditTitle')}</h3>
            <p className="text-sm text-muted-foreground mt-1">{t('devices:ctProv.stepEditDesc')}</p>
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
                  {wifiSection === 'halow'
                    ? t('devices:ble.wifiConfigHalow')
                    : t('devices:ble.wifiConfig')}
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

          <Collapsible open={advancedOpen} onOpenChange={setAdvancedOpen}>
            <div className="rounded-xl border bg-card">
              <CollapsibleTrigger className="w-full flex items-center justify-between p-4 text-left">
                <div className="flex items-center gap-2">
                  <Server className="w-4 h-4 text-primary" />
                  <span className="text-sm font-medium">{t('devices:ctProv.advancedMqtt')}</span>
                </div>
                <ChevronDown
                  className={cn(
                    'w-4 h-4 text-muted-foreground transition-transform',
                    advancedOpen && 'rotate-180',
                  )}
                />
              </CollapsibleTrigger>
              <CollapsibleContent>
                <div className="px-4 pb-4 space-y-2 border-t pt-3">
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
                  <p className="text-xs text-muted-foreground">{t('devices:ctProv.mqttAdvancedHint')}</p>
                </div>
              </CollapsibleContent>
            </div>
          </Collapsible>

          <div className="flex gap-2">
            <Button variant="outline" className="flex-1" onClick={() => setStep('capabilities')}>
              {t('common:back')}
            </Button>
            <Button
              className="flex-1"
              onClick={handleApply}
              disabled={(needsWifi && !wifiSsid) || ct.provisioning}
            >
              <ChevronRight className="w-4 h-4 mr-2" />
              {t('devices:ble.apply')}
            </Button>
          </div>
          {ct.error && <p className="text-center text-sm text-error">{ct.error}</p>}
        </div>
      )}
    </div>
  )
}

function InfoRow({ label, value }: { label: string; value: string }) {
  return (
    <div className="flex items-start justify-between gap-3">
      <span className="text-muted-foreground shrink-0">{label}</span>
      <span className="font-medium text-right break-all">{value}</span>
    </div>
  )
}
