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
import {
  formatProvUserMessage,
  modelToDeviceType,
  type CtMqttConfig,
  type NetworkScanItem,
} from '@/lib/ct-prov'
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
const WIFI_MANUAL = '__manual__'

function StepDots({ current }: { current: CtWizardStep }) {
  const { t } = useTranslation('devices')
  const idx = STEP_ORDER.indexOf(current)
  const labels = [
    t('ctProv.stepInfo'),
    t('ctProv.stepCapabilities'),
    t('ctProv.stepConfigure'),
  ]
  return (
    <ol
      className="mb-5 grid grid-cols-3 gap-0"
      aria-label={t('ctProv.wizardStepsAria')}
    >
      {STEP_ORDER.map((step, i) => {
        const active = i === idx
        const done = idx > i
        return (
          <li key={step} className="relative flex flex-col items-center gap-1.5 px-1">
            {i > 0 && (
              <span
                aria-hidden
                className={cn(
                  'pointer-events-none absolute top-3.5 right-1/2 left-[-50%] h-px',
                  done || active ? 'bg-primary' : 'bg-border',
                )}
              />
            )}
            <span
              className={cn(
                'relative z-10 flex h-7 w-7 shrink-0 items-center justify-center rounded-full text-xs font-medium',
                done && 'bg-success-light text-success',
                active && 'bg-primary text-primary-foreground',
                !done && !active && 'bg-muted text-muted-foreground',
              )}
              aria-current={active ? 'step' : undefined}
            >
              {done ? <CheckCircle2 className="h-4 w-4" aria-hidden /> : i + 1}
            </span>
            <span
              className={cn(
                'text-nano text-center leading-tight',
                active ? 'font-medium text-foreground' : 'text-muted-foreground',
              )}
            >
              {labels[i]}
            </span>
          </li>
        )
      })}
    </ol>
  )
}

function provErrorCopy(
  t: (key: string, opts?: Record<string, string>) => string,
  err: unknown,
): string {
  const code = formatProvUserMessage(err)
  if (code === 'wifi_init_failed') return t('devices:ctProv.errorWifiInit')
  if (code === 'device_busy') return t('devices:ctProv.errorBusy')
  if (code === 'not_connected') return t('devices:ctProv.errorNotConnected')
  if (code === 'internal_error') return t('devices:ctProv.errorInternal')
  return code
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
        // Fill the add-device panel so spinners sit in the visual center, not under the tab strip.
        'min-h-[min(60vh,520px)] h-full flex flex-col items-center justify-center gap-6 px-4 py-6',
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
  const [wifiNetworks, setWifiNetworks] = useState<NetworkScanItem[]>([])
  const [wifiScanning, setWifiScanning] = useState(false)
  const [wifiSelect, setWifiSelect] = useState<string>(WIFI_MANUAL)
  const [wifiScanAttempted, setWifiScanAttempted] = useState(false)
  const [wifiScanError, setWifiScanError] = useState<string | null>(null)

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
  const wifiScannable = useMemo(() => {
    if (!wifiSection) return false
    return !!ct.capabilities?.sections.find(
      (s) => s.name === wifiSection && s.configurable && s.scannable,
    )
  }, [ct.capabilities, wifiSection])

  const refreshWifiScan = useCallback(async () => {
    if (!wifiSection || !wifiScannable) return
    setWifiScanning(true)
    setWifiScanAttempted(true)
    setWifiScanError(null)
    ct.clearError()
    try {
      const items = await ct.scanWifiNetworks(wifiSection)
      setWifiNetworks(items)
      if (items.length > 0) {
        setWifiSelect((prev) => {
          if (prev !== WIFI_MANUAL && items.some((n) => n.ssid === prev)) return prev
          const top = items[0]!.ssid
          setWifiSsid(top)
          return top
        })
      } else {
        setWifiSelect(WIFI_MANUAL)
      }
    } catch (err) {
      setWifiNetworks([])
      setWifiSelect(WIFI_MANUAL)
      setWifiScanError(provErrorCopy(t, err))
    } finally {
      setWifiScanning(false)
    }
  }, [ct, wifiSection, wifiScannable, t])

  // Opt-in scan only — auto communication.scan on edit entry can leave the
  // device Wi-Fi stack in wifi_init failure before config.set (EK410/BLE).

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
    setWifiScanError(null)
    ct.clearError()

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

      await ct.applyConfig({
        wifiSsid: needsWifi ? wifiSsid : undefined,
        wifiPassword: needsWifi ? wifiPassword : undefined,
        mqtt: resp.mqtt_config,
        networkSection: wifiSection ?? undefined,
      })

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
      setRegisterError(provErrorCopy(t, err))
      setStep('failed')
    }
  }, [ct, wifiSsid, wifiPassword, deviceName, brokerId, needsWifi, wifiSection, deviceTypes, t])

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
    const failMsg = registerError || (ct.error ? provErrorCopy(t, ct.error) : null)
    return (
      <CtProvisionCenter>
        <div className="w-16 h-16 rounded-xl bg-error-light flex items-center justify-center">
          <XCircle className="w-8 h-8 text-error" />
        </div>
        <div className="text-center max-w-sm space-y-2">
          <h3 className="text-lg font-medium">{t('devices:ble.failedTitle')}</h3>
          <p className="text-sm text-muted-foreground">{failMsg}</p>
          {failureHint && (
            <p className="text-xs text-muted-foreground">{failureHint}</p>
          )}
        </div>
        <Button onClick={onReset}>{t('common:retry')}</Button>
      </CtProvisionCenter>
    )
  }

  return (
    <div className="mx-auto w-full max-w-md flex-1 flex flex-col min-h-0 py-2 sm:py-4">
      <StepDots current={step} />

      {/* Connected strip — compact, not a competing card stack */}
      <div className="mb-4 flex items-center gap-3 rounded-lg border bg-muted-30 px-3 py-2.5">
        <div className="flex h-8 w-8 shrink-0 items-center justify-center rounded-md bg-success-light">
          <ChannelIcon className="h-4 w-4 text-success" />
        </div>
        <div className="min-w-0 flex-1">
          <p className="truncate text-sm font-medium">
            {ct.deviceInfo?.deviceName || ct.deviceInfo?.model}
          </p>
          <p className="text-xs text-muted-foreground">
            <span className="text-success">{t('devices:ble.connected')}</span>
            {ct.deviceInfo?.model ? ` · ${ct.deviceInfo.model}` : ''}
          </p>
        </div>
      </div>

      <div className="flex-1 min-h-0 space-y-4 overflow-y-auto pb-2">

      {step === 'info' && (
        <div className="space-y-4">
          <div className="mb-1 text-center">
            <h3 className="text-base font-medium">{t('devices:ctProv.stepInfoTitle')}</h3>
            <p className="mt-1 text-sm text-muted-foreground">{t('devices:ctProv.stepInfoDesc')}</p>
          </div>
          <div className="space-y-3 rounded-xl border bg-card p-4 text-sm">
            <InfoRow label={t('devices:ble.deviceName')} value={ct.deviceInfo?.deviceName || '—'} />
            <InfoRow label={t('devices:ctProv.serialNumber')} value={ct.deviceInfo?.serialNumber || '—'} />
            <InfoRow label={t('devices:ctProv.model')} value={ct.deviceInfo?.model || '—'} />
            <InfoRow label={t('devices:ctProv.firmware')} value={ct.deviceInfo?.firmware || '—'} />
          </div>
        </div>
      )}

      {step === 'capabilities' && (
        <div className="space-y-4">
          <div className="mb-1 text-center">
            <h3 className="text-base font-medium">{t('devices:ctProv.stepCapsTitle')}</h3>
            <p className="mt-1 text-sm text-muted-foreground">{t('devices:ctProv.stepCapsDesc')}</p>
          </div>
          <div className="space-y-2 rounded-xl border bg-card p-4">
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
              <CollapsibleTrigger className="flex items-center gap-1 pt-1 text-xs text-muted-foreground hover:text-foreground">
                <ChevronDown
                  className={cn('h-3.5 w-3.5 transition-transform', capsDetailOpen && 'rotate-180')}
                />
                {t('devices:ctProv.showCapabilities')}
              </CollapsibleTrigger>
              <CollapsibleContent className="space-y-1 pt-2">
                {(ct.capabilities?.sections ?? []).map((s) => (
                  <div
                    key={s.name}
                    className="flex items-center justify-between rounded-lg bg-muted-30 px-2.5 py-1.5 text-xs"
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
        </div>
      )}

      {step === 'edit' && (
        <div className="space-y-4">
          <div className="mb-1 text-center">
            <h3 className="text-base font-medium">{t('devices:ctProv.stepEditTitle')}</h3>
            <p className="mt-1 text-sm text-muted-foreground">{t('devices:ctProv.stepEditDesc')}</p>
          </div>

          <div className="space-y-2">
            <Label htmlFor="ct-device-name">{t('devices:ble.deviceName')}</Label>
            <Input
              id="ct-device-name"
              value={deviceName}
              onChange={(e) => setDeviceName(e.target.value)}
              placeholder={ct.deviceInfo?.model || 'Camera'}
              className="text-base"
            />
          </div>

          {needsWifi && (
            <div className="space-y-3 rounded-xl border bg-card p-4">
              <div className="flex items-center justify-between gap-2">
                <div className="flex items-center gap-2">
                  <Wifi className="h-4 w-4 text-primary" />
                  <span className="text-sm font-medium">
                    {wifiSection === 'halow'
                      ? t('devices:ble.wifiConfigHalow')
                      : t('devices:ble.wifiConfig')}
                  </span>
                </div>
                {wifiScannable && (
                  <Button
                    type="button"
                    variant="ghost"
                    size="sm"
                    className="h-8 px-2 text-xs"
                    onClick={() => void refreshWifiScan()}
                    disabled={wifiScanning || ct.provisioning}
                  >
                    {wifiScanning ? (
                      <Loader2 className="mr-1 h-3.5 w-3.5 animate-spin" />
                    ) : (
                      <Signal className="mr-1 h-3.5 w-3.5" />
                    )}
                    {wifiScanAttempted
                      ? t('devices:ctProv.rescanWifi')
                      : t('devices:ctProv.scanWifi')}
                  </Button>
                )}
              </div>

              {wifiScannable && wifiScanning && (
                <div className="flex items-center justify-center gap-2 rounded-lg border bg-muted-30 py-5 text-sm text-muted-foreground">
                  <Loader2 className="h-4 w-4 animate-spin" />
                  {t('devices:ctProv.scanningWifi')}
                </div>
              )}

              {/* Only show the network picker when the device actually returned SSIDs */}
              {wifiScannable && !wifiScanning && wifiNetworks.length > 0 && (
                <div className="space-y-1.5">
                  <Label>{t('devices:ctProv.wifiNetwork')}</Label>
                  <Select
                    value={wifiSelect}
                    onValueChange={(v) => {
                      setWifiSelect(v)
                      if (v === WIFI_MANUAL) return
                      setWifiSsid(v)
                    }}
                  >
                    <SelectTrigger>
                      <SelectValue placeholder={t('devices:ctProv.wifiSelectPlaceholder')} />
                    </SelectTrigger>
                    <SelectContent>
                      {wifiNetworks.map((n) => (
                        <SelectItem key={`${n.ssid}:${n.bssid ?? ''}`} value={n.ssid}>
                          {n.rssi != null ? `${n.ssid} (${n.rssi} dBm)` : n.ssid}
                        </SelectItem>
                      ))}
                      <SelectItem value={WIFI_MANUAL}>
                        {t('devices:ctProv.wifiManual')}
                      </SelectItem>
                    </SelectContent>
                  </Select>
                </div>
              )}

              {wifiScannable && !wifiScanning && wifiScanAttempted && wifiNetworks.length === 0 && (
                <p className="text-xs text-muted-foreground">
                  {wifiScanError || t('devices:ctProv.wifiScanEmpty')}
                </p>
              )}

              {(!wifiScannable ||
                wifiSelect === WIFI_MANUAL ||
                (!wifiScanning && wifiNetworks.length === 0)) && (
                <div className="space-y-1.5">
                  <Label htmlFor="ct-wifi-ssid">{t('devices:ble.ssid')}</Label>
                  <Input
                    id="ct-wifi-ssid"
                    value={wifiSsid}
                    onChange={(e) => {
                      setWifiSsid(e.target.value)
                      setWifiSelect(WIFI_MANUAL)
                    }}
                    placeholder={t('devices:ble.ssidPlaceholder')}
                    autoComplete="off"
                  />
                </div>
              )}

              <div className="space-y-1.5">
                <Label htmlFor="ct-wifi-password">{t('devices:ble.password')}</Label>
                <PasswordInput
                  id="ct-wifi-password"
                  value={wifiPassword}
                  onChange={(e) => setWifiPassword(e.target.value)}
                  placeholder={t('devices:ble.passwordPlaceholder')}
                  autoComplete="new-password"
                />
              </div>
            </div>
          )}

          {!needsWifi && (
            <div className="space-y-2 rounded-xl border bg-card p-4">
              <div className="flex items-center gap-2">
                <Signal className="h-4 w-4 text-primary" />
                <span className="text-sm font-medium">{t('devices:ble.cat1NoWifi')}</span>
              </div>
              <p className="text-xs text-muted-foreground">{t('devices:ble.cat1Hint')}</p>
            </div>
          )}

          <Collapsible open={advancedOpen} onOpenChange={setAdvancedOpen}>
            <div className="rounded-xl border bg-card">
              <CollapsibleTrigger className="flex w-full items-center justify-between p-4 text-left">
                <div className="flex items-center gap-2">
                  <Server className="h-4 w-4 text-primary" />
                  <span className="text-sm font-medium">{t('devices:ctProv.advancedMqtt')}</span>
                </div>
                <ChevronDown
                  className={cn(
                    'h-4 w-4 text-muted-foreground transition-transform',
                    advancedOpen && 'rotate-180',
                  )}
                />
              </CollapsibleTrigger>
              <CollapsibleContent>
                <div className="space-y-2 border-t px-4 pb-4 pt-3">
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
                  <p className="text-xs text-muted-foreground">
                    {t('devices:ctProv.mqttAdvancedHint')}
                  </p>
                </div>
              </CollapsibleContent>
            </div>
          </Collapsible>
        </div>
      )}
      </div>

      {/* Sticky step actions — outside the scroll region */}
      {step === 'info' && (
        <div className="mt-4 shrink-0">
          <Button className="w-full" onClick={() => setStep('capabilities')}>
            <ChevronRight className="mr-2 h-4 w-4" />
            {t('devices:ctProv.continue')}
          </Button>
        </div>
      )}
      {step === 'capabilities' && (
        <div className="mt-4 flex shrink-0 gap-2">
          <Button variant="outline" className="flex-1" onClick={() => setStep('info')}>
            {t('common:back')}
          </Button>
          <Button className="flex-1" onClick={() => setStep('edit')}>
            <ChevronRight className="mr-2 h-4 w-4" />
            {t('devices:ctProv.continue')}
          </Button>
        </div>
      )}
      {step === 'edit' && (
        <div className="mt-4 shrink-0 space-y-2">
          <div className="flex gap-2">
            <Button variant="outline" className="flex-1" onClick={() => setStep('capabilities')}>
              {t('common:back')}
            </Button>
            <Button
              className="flex-1"
              onClick={handleApply}
              disabled={(needsWifi && !wifiSsid.trim()) || ct.provisioning}
            >
              <ChevronRight className="mr-2 h-4 w-4" />
              {t('devices:ble.apply')}
            </Button>
          </div>
          {(wifiScanError || ct.error) && (
            <p className="text-center text-sm text-error">
              {wifiScanError || (ct.error ? provErrorCopy(t, ct.error) : null)}
            </p>
          )}
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
