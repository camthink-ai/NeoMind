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
  Globe,
  Loader2,
  Radio,
  Server,
  Signal,
  Wifi,
  XCircle,
} from 'lucide-react'

import { Button } from '@/components/ui/button'
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from '@/components/ui/collapsible'
import { Field, FieldLabel } from '@/components/ui/field'
import { Input } from '@/components/ui/input'
import { PasswordInput } from '@/components/ui/password-input'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { Switch } from '@/components/ui/switch'
import type { UseCtProvisionReturn } from '@/hooks/useCtProvision'
import {
  cellularSectionFromFields,
  formatProvUserMessage,
  modelToDeviceType,
  normalizeCellularAuth,
  normalizeMqttQos,
  webhookSectionFromFields,
  type CellularAuth,
  type CtMqttConfig,
  type CtMqttQos,
  type CustomMqttFields,
  type NetworkScanItem,
} from '@/lib/ct-prov'
import { api, fetchAPI } from '@/lib/api'
import { useStore } from '@/store'
import { cn } from '@/lib/utils'

type BrokerSource = 'neomind' | 'custom'

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
  const n = STEP_ORDER.length
  return (
    <ol
      className="mb-5 grid grid-cols-3 gap-0"
      aria-label={t('ctProv.wizardStepsAria')}
    >
      {STEP_ORDER.map((step, i) => {
        const active = i === idx
        const done = idx > i
        const lineDone = idx >= i
        return (
          <li key={step} className="flex flex-col items-center gap-1.5 px-1">
            {/* Circle row only — connectors stay here so they never slice labels. */}
            <div className="relative flex h-7 w-full items-center justify-center">
              {i > 0 && (
                <span
                  aria-hidden
                  className={cn(
                    'pointer-events-none absolute inset-y-0 left-0 right-1/2 z-0 flex items-center',
                  )}
                >
                  <span
                    className={cn('h-px w-full', lineDone ? 'bg-primary' : 'bg-border')}
                  />
                </span>
              )}
              {i < n - 1 && (
                <span
                  aria-hidden
                  className={cn(
                    'pointer-events-none absolute inset-y-0 left-1/2 right-0 z-0 flex items-center',
                  )}
                >
                  <span
                    className={cn(
                      'h-px w-full',
                      idx > i ? 'bg-primary' : 'bg-border',
                    )}
                  />
                </span>
              )}
              {/* Opaque chip fills — success-light / muted-tints are translucent
                  and let the connector show through numbers/icons. */}
              <span
                className={cn(
                  'relative z-10 flex h-7 w-7 shrink-0 items-center justify-center rounded-full text-xs font-medium',
                  done && 'bg-card text-success ring-2 ring-inset ring-success',
                  active && 'bg-primary text-primary-foreground',
                  !done && !active && 'bg-card text-muted-foreground ring-1 ring-inset ring-border',
                )}
                aria-current={active ? 'step' : undefined}
              >
                {done ? <CheckCircle2 className="h-4 w-4" aria-hidden /> : i + 1}
              </span>
            </div>
            <span
              className={cn(
                'relative z-10 max-w-full rounded-sm bg-background px-0.5 text-center text-nano leading-tight',
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
  opts?: { forScan?: boolean },
): string {
  const code = formatProvUserMessage(err)
  if (code === 'wifi_init_failed') {
    // Scan path: emphasize manual SSID still works. Apply path keeps the fuller reboot hint.
    return opts?.forScan
      ? t('devices:ctProv.errorWifiInitScan')
      : t('devices:ctProv.errorWifiInit')
  }
  if (code === 'device_busy') return t('devices:ctProv.errorBusy')
  if (code === 'not_connected') return t('devices:ctProv.errorNotConnected')
  if (code === 'internal_error') return t('devices:ctProv.errorInternal')
  return code
}

/** Shared status/hero tile for CT-Prov center states (scan/connect/apply/done/fail). */
export function CtProvisionHero({
  tone = 'primary',
  size = 'lg',
  children,
}: {
  tone?: 'primary' | 'success' | 'error'
  /**
   * First-viewport SoftAP/BLE/Manual/Auto language: `md` = w-14 (matches Manual/Auto).
   * `lg` = status states. `sm` kept for rare compact call sites.
   */
  size?: 'sm' | 'md' | 'lg'
  children: ReactNode
}) {
  return (
    <div
      className={cn(
        'mx-auto flex shrink-0 items-center justify-center rounded-xl',
        size === 'sm' && 'h-10 w-10',
        size === 'md' && 'h-14 w-14',
        size === 'lg' && 'h-16 w-16',
        tone === 'primary' && 'bg-primary-light',
        tone === 'success' && 'bg-success-light',
        tone === 'error' && 'bg-error-light',
      )}
    >
      {children}
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
        // Mid-pane center kept; hero spacing matches Manual/Auto density (gap-6).
        // w-full + flex-1: without width, a shrink-wrapped column can sit left in the
        // pane when a parent uses items-center; flex-1 keeps vertical fill reliable.
        // Horizontal padding comes from AddDeviceGlobalDialog — avoid double inset.
        'flex h-full min-h-0 w-full flex-1 flex-col items-center justify-center gap-6 py-6',
        className,
      )}
    >
      {children}
    </div>
  )
}

function StepActions({ children }: { children: ReactNode }) {
  // Flat on the pane (no tinted strip). Border only separates sticky actions
  // when the step body scrolls underneath.
  return (
    <div className="mt-auto shrink-0 border-t border-border pt-3">
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
  const [brokerSource, setBrokerSource] = useState<BrokerSource>('neomind')
  const [customEnable, setCustomEnable] = useState(true)
  const [customHost, setCustomHost] = useState('')
  const [customPort, setCustomPort] = useState('1883')
  const [customTls, setCustomTls] = useState(false)
  const [customUsername, setCustomUsername] = useState('')
  const [customPassword, setCustomPassword] = useState('')
  const [customClientId, setCustomClientId] = useState('')
  const [customTopic, setCustomTopic] = useState('')
  const [customQos, setCustomQos] = useState<CtMqttQos>(1)
  const [mqttFormError, setMqttFormError] = useState<string | null>(null)
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

  // Cellular (capabilities-gated)
  const [cellApn, setCellApn] = useState('')
  const [cellUsername, setCellUsername] = useState('')
  const [cellPassword, setCellPassword] = useState('')
  const [cellPin, setCellPin] = useState('')
  const [cellAuth, setCellAuth] = useState<CellularAuth>('none')
  const [cellRoaming, setCellRoaming] = useState(false)
  const [cellFormError, setCellFormError] = useState<string | null>(null)

  // Webhook (capabilities-gated)
  const [webhookEnable, setWebhookEnable] = useState(false)
  const [webhookUrl, setWebhookUrl] = useState('')
  const [webhookSecret, setWebhookSecret] = useState('')
  const [webhookHeader, setWebhookHeader] = useState('')
  const [webhookFormError, setWebhookFormError] = useState<string | null>(null)

  const selectBrokerSource = useCallback((source: BrokerSource) => {
    setBrokerSource(source)
    setMqttFormError(null)
    if (source === 'custom') setAdvancedOpen(true)
  }, [])

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

  const cellularSection = useMemo(() => {
    return ct.capabilities?.sections.find((s) => s.name === 'cellular' && s.configurable) ?? null
  }, [ct.capabilities])
  const cellularFields = useMemo(() => cellularSection?.fields ?? [], [cellularSection])
  const needsCellular = cellularSection !== null

  const webhookSection = useMemo(() => {
    return ct.capabilities?.sections.find((s) => s.name === 'webhook' && s.configurable) ?? null
  }, [ct.capabilities])
  const webhookFields = useMemo(() => webhookSection?.fields ?? [], [webhookSection])
  const needsWebhook = webhookSection !== null

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
      // Device-side wifi_init (common on some BLE firmwares) — not a NeoMind wire bug.
      // Keep the session usable: fall through to manual SSID fields.
      setWifiScanError(provErrorCopy(t, err, { forScan: true }))
    } finally {
      setWifiScanning(false)
    }
  }, [ct, wifiSection, wifiScannable, t])

  // Opt-in scan only — auto communication.scan on edit entry can leave the
  // device Wi-Fi stack in wifi_init failure before config.set (EK410/BLE).

  const buildCustomMqtt = useCallback((): CustomMqttFields | null => {
    const qos = normalizeMqttQos(customQos, 1)
    const topic = customTopic.trim()
    const clientId = customClientId.trim()

    // enable=false is a complete CT mqtt write (device clears broker); host optional.
    if (!customEnable) {
      setMqttFormError(null)
      const portNum = Number.parseInt(customPort.trim() || '1883', 10)
      return {
        enable: false,
        host: customHost.trim() || '0.0.0.0',
        port: Number.isFinite(portNum) && portNum >= 1 && portNum <= 65535 ? portNum : 1883,
        username: customUsername.trim() || undefined,
        password: customPassword || undefined,
        tls: customTls,
        qos,
        topic_prefix: topic || undefined,
        client_id: clientId || undefined,
      }
    }

    const host = customHost.trim()
    if (!host) {
      setMqttFormError(t('devices:ctProv.customHostRequired'))
      return null
    }
    const portNum = Number.parseInt(customPort.trim(), 10)
    if (!Number.isFinite(portNum) || portNum < 1 || portNum > 65535) {
      setMqttFormError(t('devices:ctProv.customPortInvalid'))
      return null
    }
    setMqttFormError(null)
    return {
      enable: true,
      host,
      port: portNum,
      username: customUsername.trim() || undefined,
      password: customPassword || undefined,
      tls: customTls,
      qos,
      topic_prefix: topic || undefined,
      client_id: clientId || undefined,
    }
  }, [
    customEnable,
    customHost,
    customPort,
    customTls,
    customUsername,
    customPassword,
    customTopic,
    customClientId,
    customQos,
    t,
  ])

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

    let customMqtt: CustomMqttFields | undefined
    if (brokerSource === 'custom') {
      const built = buildCustomMqtt()
      if (!built) {
        setAdvancedOpen(true)
        return
      }
      customMqtt = built
    }

    setCellFormError(null)
    setWebhookFormError(null)

    let cellularPayload: Record<string, unknown> | undefined
    if (needsCellular) {
      if (cellularFields.includes('apn') && !cellApn.trim()) {
        // apn declared → required when the operator opens cellular config.
        // Pre-provisioned SIMs omit apn from fields entirely.
        setCellFormError(t('devices:ctProv.cellularApnRequired'))
        return
      }
      if (cellularFields.includes('pin') && cellPin.trim()) {
        if (!/^\d{4,8}$/.test(cellPin.trim())) {
          setCellFormError(t('devices:ctProv.cellularPinInvalid'))
          return
        }
      }
      const built = cellularSectionFromFields(
        {
          apn: cellApn,
          username: cellUsername,
          password: cellPassword,
          pin: cellPin,
          auth: cellAuth,
          roaming: cellRoaming,
        },
        cellularFields,
      )
      if (built) cellularPayload = built
    }

    let webhookPayload: Record<string, unknown> | undefined
    if (needsWebhook) {
      if (webhookEnable) {
        const url = webhookUrl.trim()
        if (!url) {
          setWebhookFormError(t('devices:ctProv.webhookUrlRequired'))
          return
        }
        try {
          const parsed = new URL(url)
          if (parsed.protocol !== 'http:' && parsed.protocol !== 'https:') {
            setWebhookFormError(t('devices:ctProv.webhookUrlInvalid'))
            return
          }
        } catch {
          setWebhookFormError(t('devices:ctProv.webhookUrlInvalid'))
          return
        }
        if (webhookFields.includes('header') && webhookHeader.trim()) {
          const h = webhookHeader.trim()
          if (!h.includes(':') || !h.split(':')[0]?.trim()) {
            setWebhookFormError(t('devices:ctProv.webhookHeaderInvalid'))
            return
          }
        }
      }
      const built = webhookSectionFromFields(
        {
          enable: webhookEnable,
          url: webhookUrl,
          secret: webhookSecret,
          header: webhookHeader,
        },
        webhookFields,
      )
      if (built) webhookPayload = built
    }

    setStep('applying')
    setRegisterError(null)
    setWifiScanError(null)
    setMqttFormError(null)
    ct.clearError()

    const provisionBody = {
      model: modelName,
      sn,
      device_type: deviceType,
      device_name: name,
      broker_id: brokerSource === 'custom' ? 'custom' : brokerId,
      ...(customMqtt ? { custom_mqtt: customMqtt } : {}),
    }

    try {
      const resp = await fetchAPI<{ device_id: string; mqtt_config: CtMqttConfig }>(
        '/devices/ble-provision',
        {
          method: 'POST',
          body: JSON.stringify({ ...provisionBody, resolve_only: true }),
        },
      )

      await ct.applyConfig({
        wifiSsid: needsWifi ? wifiSsid : undefined,
        wifiPassword: needsWifi ? wifiPassword : undefined,
        mqtt: resp.mqtt_config,
        networkSection: wifiSection ?? undefined,
        cellular: cellularPayload,
        webhook: webhookPayload,
      })

      try {
        const regResp = await fetchAPI<{ device_id: string; already_exists?: boolean }>(
          '/devices/ble-provision',
          {
            method: 'POST',
            body: JSON.stringify({ ...provisionBody, resolve_only: false }),
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
  }, [
    ct,
    wifiSsid,
    wifiPassword,
    deviceName,
    brokerId,
    brokerSource,
    buildCustomMqtt,
    needsWifi,
    wifiSection,
    needsCellular,
    cellularFields,
    cellApn,
    cellUsername,
    cellPassword,
    cellPin,
    cellAuth,
    cellRoaming,
    needsWebhook,
    webhookFields,
    webhookEnable,
    webhookUrl,
    webhookSecret,
    webhookHeader,
    deviceTypes,
    t,
  ])

  if (step === 'applying') {
    return (
      <CtProvisionCenter>
        <CtProvisionHero>
          <Loader2 className="h-7 w-7 animate-spin text-primary" />
        </CtProvisionHero>
        <div className="space-y-1 text-center">
          <h3 className="text-base font-medium">{t('devices:ble.provisioning')}</h3>
          <p className="text-sm text-muted-foreground">
            {ct.provisioningStep === 'writingConfig' && t('devices:ble.status.writingMqtt')}
            {ct.provisioningStep === 'closing' && t('devices:ctProv.closingSession')}
            {ct.provisioningStep === 'done' && t('devices:ble.status.done')}
            {!ct.provisioningStep && t('devices:ctProv.applyingHint')}
          </p>
        </div>
        <Button variant="outline" onClick={onReset}>
          {t('common:cancel')}
        </Button>
      </CtProvisionCenter>
    )
  }

  if (step === 'done') {
    return (
      <CtProvisionCenter>
        <CtProvisionHero tone="success">
          <CheckCircle2 className="h-7 w-7 text-success" />
        </CtProvisionHero>
        <div className="space-y-0.5 text-center">
          <h3 className="text-base font-medium">
            {isReprovision ? t('devices:ble.reprovisionTitle') : t('devices:ble.successTitle')}
          </h3>
          <p className="text-sm text-muted-foreground">
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
        <CtProvisionHero tone="error">
          <XCircle className="h-7 w-7 text-error" />
        </CtProvisionHero>
        <div className="max-w-sm space-y-1 text-center">
          <h3 className="text-base font-medium">{t('devices:ble.failedTitle')}</h3>
          <p className="text-sm text-muted-foreground">{failMsg}</p>
          {failureHint && <p className="text-xs text-muted-foreground">{failureHint}</p>}
        </div>
        <Button onClick={onReset}>{t('common:retry')}</Button>
      </CtProvisionCenter>
    )
  }

  return (
    <div className="mx-auto flex h-full min-h-0 w-full max-w-3xl flex-1 flex-col self-center py-4">
      <StepDots current={step} />

      {/* Connected strip — compact, not a competing card stack */}
      <div className="mb-4 flex items-center gap-3 rounded-lg border border-border bg-muted-30 px-3 py-2.5">
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

      <div className="min-h-0 flex-1 space-y-4 overflow-y-auto pb-3">
        {step === 'info' && (
          <div className="space-y-4">
            <div className="space-y-1 text-center">
              <h3 className="text-base font-medium">{t('devices:ctProv.stepInfoTitle')}</h3>
              <p className="text-sm text-muted-foreground">{t('devices:ctProv.stepInfoDesc')}</p>
            </div>
            <div className="space-y-3 rounded-xl border border-border p-4 text-sm">
              <InfoRow label={t('devices:ble.deviceName')} value={ct.deviceInfo?.deviceName || '—'} />
              <InfoRow
                label={t('devices:ctProv.serialNumber')}
                value={ct.deviceInfo?.serialNumber || '—'}
              />
              <InfoRow label={t('devices:ctProv.model')} value={ct.deviceInfo?.model || '—'} />
              <InfoRow label={t('devices:ctProv.firmware')} value={ct.deviceInfo?.firmware || '—'} />
            </div>
          </div>
        )}

        {step === 'capabilities' && (
          <div className="space-y-4">
            <div className="space-y-1 text-center">
              <h3 className="text-base font-medium">{t('devices:ctProv.stepCapsTitle')}</h3>
              <p className="text-sm text-muted-foreground">{t('devices:ctProv.stepCapsDesc')}</p>
            </div>
            <div className="space-y-3 rounded-xl border border-border p-4">
              {/* Operator checklist — not raw protocol section names. */}
              <ul className="space-y-2.5">
                {needsWifi && (
                  <CapsChecklistItem
                    icon={Wifi}
                    title={
                      wifiSection === 'halow'
                        ? t('devices:ctProv.capsItemHalow')
                        : t('devices:ctProv.capsItemWifi')
                    }
                    hint={t('devices:ctProv.capsItemWifiHint')}
                  />
                )}
                {needsCellular && (
                  <CapsChecklistItem
                    icon={Radio}
                    title={t('devices:ctProv.capsItemCellular')}
                    hint={t('devices:ctProv.capsItemCellularHint')}
                  />
                )}
                {!needsWifi && !needsCellular && (
                  <CapsChecklistItem
                    icon={Signal}
                    title={t('devices:ctProv.capsItemNoWifi')}
                    hint={t('devices:ble.cat1Hint')}
                  />
                )}
                <CapsChecklistItem
                  icon={Server}
                  title={t('devices:ctProv.capsItemMqtt')}
                  hint={t('devices:ctProv.capsItemMqttHint')}
                />
                {needsWebhook && (
                  <CapsChecklistItem
                    icon={Globe}
                    title={t('devices:ctProv.capsItemWebhook')}
                    hint={t('devices:ctProv.capsItemWebhookHint')}
                  />
                )}
              </ul>
              {ct.capabilities?.currentNetwork && (
                <p className="text-xs text-muted-foreground">
                  {t('devices:ctProv.currentNetwork', {
                    network: humanizeNetworkName(ct.capabilities.currentNetwork, t),
                  })}
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
                  <p className="pb-1 text-xs text-muted-foreground">
                    {t('devices:ctProv.capsTechnicalHint')}
                  </p>
                  {(ct.capabilities?.sections ?? []).map((s) => (
                    <div
                      key={s.name}
                      className="flex items-center justify-between rounded-lg bg-muted-30 px-2.5 py-1.5 text-xs"
                    >
                      <span className="font-medium">
                        {humanizeSectionName(s.name, t)}
                      </span>
                      <span className="text-muted-foreground">
                        {s.configurable
                          ? t('devices:ctProv.configurable')
                          : t('devices:ctProv.readOnly')}
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
            <div className="space-y-1 text-center">
              <h3 className="text-base font-medium">{t('devices:ctProv.stepEditTitle')}</h3>
              <p className="text-sm text-muted-foreground">{t('devices:ctProv.stepEditDesc')}</p>
            </div>

            <Field>
              <FieldLabel htmlFor="ct-device-name">{t('devices:ble.deviceName')}</FieldLabel>
              <Input
                id="ct-device-name"
                value={deviceName}
                onChange={(e) => setDeviceName(e.target.value)}
                placeholder={ct.deviceInfo?.model || 'Camera'}
                className="text-base"
              />
            </Field>

            {needsWifi && (
              <div className="space-y-3 rounded-xl border border-border p-4">
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
                  <div className="flex items-center justify-center gap-2 rounded-lg border border-border bg-muted-30 py-5 text-sm text-muted-foreground">
                    <Loader2 className="h-4 w-4 animate-spin" />
                    {t('devices:ctProv.scanningWifi')}
                  </div>
                )}

                {/* Visible SSID list after scan — not a closed Select (results must stay on-screen). */}
                {wifiScannable && !wifiScanning && wifiNetworks.length > 0 && (
                  <Field>
                    <FieldLabel>{t('devices:ctProv.wifiNetwork')}</FieldLabel>
                    <div
                      role="listbox"
                      aria-label={t('devices:ctProv.wifiNetwork')}
                      className="max-h-48 space-y-1 overflow-y-auto rounded-lg border border-border p-1"
                    >
                      {wifiNetworks.map((n) => {
                        const selected = wifiSelect === n.ssid
                        const label = n.displayName || n.ssid
                        return (
                          <button
                            key={`${n.ssid}:${n.bssid ?? n.channel ?? ''}`}
                            type="button"
                            role="option"
                            aria-selected={selected}
                            className={cn(
                              'flex w-full items-center justify-between gap-2 rounded-md px-3 py-2 text-left text-sm transition-colors',
                              selected
                                ? 'bg-primary text-primary-foreground'
                                : 'hover:bg-muted-30',
                            )}
                            onClick={() => {
                              setWifiSelect(n.ssid)
                              setWifiSsid(n.ssid)
                            }}
                          >
                            <span className="min-w-0 truncate font-medium">{label}</span>
                            {n.rssi != null && (
                              <span
                                className={cn(
                                  'shrink-0 text-xs tabular-nums',
                                  selected ? 'text-primary-foreground' : 'text-muted-foreground',
                                )}
                              >
                                {n.rssi} dBm
                              </span>
                            )}
                          </button>
                        )
                      })}
                      <button
                        type="button"
                        role="option"
                        aria-selected={wifiSelect === WIFI_MANUAL}
                        className={cn(
                          'flex w-full items-center rounded-md px-3 py-2 text-left text-sm transition-colors',
                          wifiSelect === WIFI_MANUAL
                            ? 'bg-primary text-primary-foreground'
                            : 'hover:bg-muted-30 text-muted-foreground',
                        )}
                        onClick={() => setWifiSelect(WIFI_MANUAL)}
                      >
                        {t('devices:ctProv.wifiManual')}
                      </button>
                    </div>
                  </Field>
                )}

                {wifiScannable && !wifiScanning && wifiScanAttempted && wifiNetworks.length === 0 && (
                  <div
                    className="space-y-1 rounded-lg border border-border bg-muted-30 px-3 py-2.5"
                    role="status"
                  >
                    <p className="text-sm font-medium text-foreground">
                      {wifiScanError
                        ? t('devices:ctProv.wifiScanFailedTitle')
                        : t('devices:ctProv.wifiScanEmpty')}
                    </p>
                    {wifiScanError ? (
                      <>
                        <p className="text-xs leading-relaxed text-muted-foreground">
                          {wifiScanError}
                        </p>
                        <p className="text-xs font-medium text-foreground">
                          {t('devices:ctProv.wifiManualFallbackHint')}
                        </p>
                      </>
                    ) : null}
                  </div>
                )}

                {(!wifiScannable ||
                  wifiSelect === WIFI_MANUAL ||
                  (!wifiScanning && wifiNetworks.length === 0)) && (
                  <Field>
                    <FieldLabel htmlFor="ct-wifi-ssid">{t('devices:ble.ssid')}</FieldLabel>
                    <Input
                      id="ct-wifi-ssid"
                      value={wifiSsid}
                      onChange={(e) => {
                        setWifiSsid(e.target.value)
                        setWifiSelect(WIFI_MANUAL)
                      }}
                      placeholder={t('devices:ble.ssidPlaceholder')}
                      autoComplete="off"
                      autoFocus={!!wifiScanError}
                    />
                  </Field>
                )}

                <Field>
                  <FieldLabel htmlFor="ct-wifi-password">{t('devices:ble.password')}</FieldLabel>
                  <PasswordInput
                    id="ct-wifi-password"
                    value={wifiPassword}
                    onChange={(e) => setWifiPassword(e.target.value)}
                    placeholder={t('devices:ble.passwordPlaceholder')}
                    autoComplete="new-password"
                  />
                </Field>
              </div>
            )}

            {needsCellular && (
              <div className="space-y-3 rounded-xl border border-border p-4">
                <div className="flex items-center gap-2">
                  <Radio className="h-4 w-4 text-primary" />
                  <span className="text-sm font-medium">{t('devices:ctProv.cellularConfig')}</span>
                </div>
                <p className="text-xs text-muted-foreground">{t('devices:ctProv.cellularHint')}</p>
                {cellularFields.includes('apn') && (
                  <Field>
                    <FieldLabel htmlFor="ct-cell-apn">{t('devices:ctProv.cellularApn')}</FieldLabel>
                    <Input
                      id="ct-cell-apn"
                      value={cellApn}
                      onChange={(e) => {
                        setCellApn(e.target.value)
                        setCellFormError(null)
                      }}
                      placeholder="cmiot.iot"
                      autoComplete="off"
                    />
                  </Field>
                )}
                {(cellularFields.includes('username') || cellularFields.includes('password')) && (
                  <div className="grid gap-3 sm:grid-cols-2">
                    {cellularFields.includes('username') && (
                      <Field>
                        <FieldLabel htmlFor="ct-cell-user">
                          {t('devices:ble.username')}
                        </FieldLabel>
                        <Input
                          id="ct-cell-user"
                          value={cellUsername}
                          onChange={(e) => setCellUsername(e.target.value)}
                          autoComplete="off"
                        />
                      </Field>
                    )}
                    {cellularFields.includes('password') && (
                      <Field>
                        <FieldLabel htmlFor="ct-cell-pass">
                          {t('devices:ble.password')}
                        </FieldLabel>
                        <PasswordInput
                          id="ct-cell-pass"
                          value={cellPassword}
                          onChange={(e) => setCellPassword(e.target.value)}
                          autoComplete="new-password"
                        />
                      </Field>
                    )}
                  </div>
                )}
                {cellularFields.includes('pin') && (
                  <Field>
                    <FieldLabel htmlFor="ct-cell-pin">{t('devices:ctProv.cellularPin')}</FieldLabel>
                    <Input
                      id="ct-cell-pin"
                      inputMode="numeric"
                      value={cellPin}
                      onChange={(e) => {
                        setCellPin(e.target.value)
                        setCellFormError(null)
                      }}
                      placeholder="1234"
                      autoComplete="off"
                    />
                    <p className="text-xs text-muted-foreground">
                      {t('devices:ctProv.cellularPinHint')}
                    </p>
                  </Field>
                )}
                {cellularFields.includes('auth') && (
                  <Field>
                    <FieldLabel>{t('devices:ctProv.cellularAuth')}</FieldLabel>
                    <Select
                      value={cellAuth}
                      onValueChange={(v) => setCellAuth(normalizeCellularAuth(v))}
                    >
                      <SelectTrigger>
                        <SelectValue />
                      </SelectTrigger>
                      <SelectContent>
                        <SelectItem value="none">{t('devices:ctProv.cellularAuthNone')}</SelectItem>
                        <SelectItem value="pap">PAP</SelectItem>
                        <SelectItem value="chap">CHAP</SelectItem>
                        <SelectItem value="pap_or_chap">
                          {t('devices:ctProv.cellularAuthPapOrChap')}
                        </SelectItem>
                      </SelectContent>
                    </Select>
                  </Field>
                )}
                {cellularFields.includes('roaming') && (
                  <div className="flex items-center justify-between gap-3 py-1">
                    <FieldLabel htmlFor="ct-cell-roaming" className="mb-0">
                      {t('devices:ctProv.cellularRoaming')}
                    </FieldLabel>
                    <Switch
                      id="ct-cell-roaming"
                      checked={cellRoaming}
                      onCheckedChange={setCellRoaming}
                    />
                  </div>
                )}
                {cellFormError && <p className="text-sm text-error">{cellFormError}</p>}
              </div>
            )}

            {!needsWifi && !needsCellular && (
              <div className="space-y-2 rounded-xl border border-border p-4">
                <div className="flex items-center gap-2">
                  <Signal className="h-4 w-4 text-primary" />
                  <span className="text-sm font-medium">{t('devices:ble.cat1NoWifi')}</span>
                </div>
                <p className="text-xs text-muted-foreground">{t('devices:ble.cat1Hint')}</p>
              </div>
            )}

            {needsWebhook && (
              <div className="space-y-3 rounded-xl border border-border p-4">
                <div className="flex items-center justify-between gap-2">
                  <div className="flex items-center gap-2">
                    <Globe className="h-4 w-4 text-primary" />
                    <span className="text-sm font-medium">{t('devices:ctProv.webhookConfig')}</span>
                  </div>
                  {webhookFields.includes('enable') && (
                    <Switch
                      id="ct-webhook-enable"
                      checked={webhookEnable}
                      onCheckedChange={(v) => {
                        setWebhookEnable(v)
                        setWebhookFormError(null)
                      }}
                    />
                  )}
                </div>
                <p className="text-xs text-muted-foreground">{t('devices:ctProv.webhookHint')}</p>
                {webhookFields.includes('url') && (
                  <Field>
                    <FieldLabel htmlFor="ct-webhook-url">
                      {t('devices:ctProv.webhookUrl')}
                    </FieldLabel>
                    <Input
                      id="ct-webhook-url"
                      value={webhookUrl}
                      onChange={(e) => {
                        setWebhookUrl(e.target.value)
                        setWebhookFormError(null)
                      }}
                      placeholder="https://example.com/hook"
                      disabled={webhookFields.includes('enable') && !webhookEnable}
                      className="font-mono text-sm"
                      autoComplete="off"
                    />
                  </Field>
                )}
                {webhookFields.includes('secret') && (
                  <Field>
                    <FieldLabel htmlFor="ct-webhook-secret">
                      {t('devices:ctProv.webhookSecret')}
                    </FieldLabel>
                    <PasswordInput
                      id="ct-webhook-secret"
                      value={webhookSecret}
                      onChange={(e) => setWebhookSecret(e.target.value)}
                      disabled={webhookFields.includes('enable') && !webhookEnable}
                      autoComplete="new-password"
                    />
                  </Field>
                )}
                {webhookFields.includes('header') && (
                  <Field>
                    <FieldLabel htmlFor="ct-webhook-header">
                      {t('devices:ctProv.webhookHeader')}
                    </FieldLabel>
                    <Input
                      id="ct-webhook-header"
                      value={webhookHeader}
                      onChange={(e) => {
                        setWebhookHeader(e.target.value)
                        setWebhookFormError(null)
                      }}
                      placeholder="X-API-Key: abc123"
                      disabled={webhookFields.includes('enable') && !webhookEnable}
                      className="font-mono text-sm"
                      autoComplete="off"
                    />
                    <p className="text-xs text-muted-foreground">
                      {t('devices:ctProv.webhookHeaderHint')}
                    </p>
                  </Field>
                )}
                {webhookFormError && <p className="text-sm text-error">{webhookFormError}</p>}
              </div>
            )}

            <Collapsible open={advancedOpen} onOpenChange={setAdvancedOpen}>
              <div className="rounded-xl border border-border">
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
                  <div className="space-y-3 border-t border-border px-4 pb-4 pt-3">
                    <div className="grid grid-cols-2 rounded-lg bg-muted p-1">
                      {(
                        [
                          { value: 'neomind' as const, label: t('devices:ctProv.brokerSourceNeomind') },
                          { value: 'custom' as const, label: t('devices:ctProv.brokerSourceCustom') },
                        ] as const
                      ).map(({ value, label }) => {
                        const isActive = brokerSource === value
                        return (
                          <button
                            key={value}
                            type="button"
                            onClick={() => selectBrokerSource(value)}
                            className={cn(
                              'min-h-11 rounded-md px-3 text-sm font-medium transition-colors',
                              isActive
                                ? 'bg-background text-foreground shadow-sm'
                                : 'text-muted-foreground',
                            )}
                          >
                            {label}
                          </button>
                        )
                      })}
                    </div>

                    {brokerSource === 'neomind' ? (
                      <div className="space-y-3">
                        <Field>
                          <FieldLabel>{t('devices:ble.broker')}</FieldLabel>
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
                        </Field>
                        <p className="text-xs text-muted-foreground">
                          {t('devices:ctProv.neomindMqttFullHint')}
                        </p>
                      </div>
                    ) : (
                      <div className="space-y-3">
                        {/* CT-Prov/1 mqtt fields: enable, host, port, client_id,
                            username, password, tls, topic, qos */}
                        <div className="flex items-center justify-between gap-3 py-1">
                          <FieldLabel htmlFor="ct-mqtt-enable" className="mb-0">
                            {t('devices:ctProv.customEnable')}
                          </FieldLabel>
                          <Switch
                            id="ct-mqtt-enable"
                            checked={customEnable}
                            onCheckedChange={setCustomEnable}
                          />
                        </div>
                        <div className="grid gap-3 sm:grid-cols-2">
                          <Field>
                            <FieldLabel htmlFor="ct-mqtt-host">
                              {t('devices:ctProv.customHost')}
                            </FieldLabel>
                            <Input
                              id="ct-mqtt-host"
                              value={customHost}
                              onChange={(e) => {
                                setCustomHost(e.target.value)
                                setMqttFormError(null)
                              }}
                              placeholder="mqtt.example.com"
                              autoComplete="off"
                            />
                          </Field>
                          <Field>
                            <FieldLabel htmlFor="ct-mqtt-port">
                              {t('devices:ctProv.customPort')}
                            </FieldLabel>
                            <Input
                              id="ct-mqtt-port"
                              inputMode="numeric"
                              value={customPort}
                              onChange={(e) => {
                                setCustomPort(e.target.value)
                                setMqttFormError(null)
                              }}
                              placeholder="1883"
                              autoComplete="off"
                            />
                          </Field>
                        </div>
                        <Field>
                          <FieldLabel htmlFor="ct-mqtt-client-id">
                            {t('devices:ctProv.customClientId')}
                          </FieldLabel>
                          <Input
                            id="ct-mqtt-client-id"
                            value={customClientId}
                            onChange={(e) => setCustomClientId(e.target.value)}
                            className="font-mono text-sm"
                            autoComplete="off"
                          />
                          <p className="text-xs text-muted-foreground">
                            {t('devices:ctProv.customClientIdHint')}
                          </p>
                        </Field>
                        <div className="grid gap-3 sm:grid-cols-2">
                          <Field>
                            <FieldLabel htmlFor="ct-mqtt-username">
                              {t('devices:ble.username')}
                            </FieldLabel>
                            <Input
                              id="ct-mqtt-username"
                              value={customUsername}
                              onChange={(e) => setCustomUsername(e.target.value)}
                              autoComplete="off"
                            />
                          </Field>
                          <Field>
                            <FieldLabel htmlFor="ct-mqtt-password">
                              {t('devices:ble.password')}
                            </FieldLabel>
                            <PasswordInput
                              id="ct-mqtt-password"
                              value={customPassword}
                              onChange={(e) => setCustomPassword(e.target.value)}
                              autoComplete="new-password"
                            />
                          </Field>
                        </div>
                        <div className="space-y-1.5 border-y border-border py-3">
                          <div className="flex items-center justify-between gap-3">
                            <FieldLabel htmlFor="ct-mqtt-tls" className="mb-0">
                              {t('devices:ctProv.customTls')}
                            </FieldLabel>
                            <Switch
                              id="ct-mqtt-tls"
                              checked={customTls}
                              onCheckedChange={(checked) => {
                                setCustomTls(checked)
                                if (checked && (customPort === '1883' || customPort.trim() === '')) {
                                  setCustomPort('8883')
                                } else if (!checked && customPort === '8883') {
                                  setCustomPort('1883')
                                }
                              }}
                            />
                          </div>
                          {customTls && (
                            <p className="text-xs text-muted-foreground">
                              {t('devices:ctProv.customTlsHint')}
                            </p>
                          )}
                        </div>
                        <Field>
                          <FieldLabel htmlFor="ct-mqtt-topic">
                            {t('devices:ctProv.customTopic')}
                          </FieldLabel>
                          <Input
                            id="ct-mqtt-topic"
                            value={customTopic}
                            onChange={(e) => setCustomTopic(e.target.value)}
                            placeholder="device/{type}/{id}/uplink"
                            className="font-mono text-sm"
                            autoComplete="off"
                          />
                          <p className="text-xs text-muted-foreground">
                            {t('devices:ctProv.customTopicHint')}
                          </p>
                        </Field>
                        <Field>
                          <FieldLabel>{t('devices:ctProv.customQos')}</FieldLabel>
                          <Select
                            value={String(customQos)}
                            onValueChange={(v) => setCustomQos(normalizeMqttQos(Number(v), 1))}
                          >
                            <SelectTrigger>
                              <SelectValue />
                            </SelectTrigger>
                            <SelectContent>
                              <SelectItem value="0">{t('devices:ctProv.customQos0')}</SelectItem>
                              <SelectItem value="1">{t('devices:ctProv.customQos1')}</SelectItem>
                              <SelectItem value="2">{t('devices:ctProv.customQos2')}</SelectItem>
                            </SelectContent>
                          </Select>
                          <p className="text-xs text-muted-foreground">
                            {t('devices:ctProv.customQosHint')}
                          </p>
                        </Field>
                      </div>
                    )}

                    <p className="text-xs text-muted-foreground">
                      {t('devices:ctProv.mqttAdvancedHint')}
                    </p>
                    {mqttFormError && (
                      <p className="text-sm text-error">{mqttFormError}</p>
                    )}
                  </div>
                </CollapsibleContent>
              </div>
            </Collapsible>
          </div>
        )}
      </div>

      {/* Sticky step actions — outside the scroll region (Main must be overflow-hidden). */}
      {step === 'info' && (
        <StepActions>
          <div className="flex items-center gap-2">
            <Button variant="outline" className="flex-1" onClick={onReset}>
              {t('common:cancel')}
            </Button>
            <Button className="flex-1" onClick={() => setStep('capabilities')}>
              <ChevronRight className="mr-2 h-4 w-4" />
              {t('devices:ctProv.continue')}
            </Button>
          </div>
        </StepActions>
      )}
      {step === 'capabilities' && (
        <StepActions>
          <div className="flex items-center gap-2">
            <Button variant="outline" onClick={() => setStep('info')}>
              {t('common:back')}
            </Button>
            <Button variant="ghost" onClick={onReset}>
              {t('common:cancel')}
            </Button>
            <Button className="min-w-0 flex-1" onClick={() => setStep('edit')}>
              <ChevronRight className="mr-2 h-4 w-4" />
              {t('devices:ctProv.continue')}
            </Button>
          </div>
        </StepActions>
      )}
      {step === 'edit' && (
        <StepActions>
          <div className="space-y-2">
            <div className="flex items-center gap-2">
              <Button variant="outline" onClick={() => setStep('capabilities')}>
                {t('common:back')}
              </Button>
              <Button variant="ghost" onClick={onReset}>
                {t('common:cancel')}
              </Button>
              <Button
                className="min-w-0 flex-1"
                onClick={handleApply}
                disabled={
                  (needsWifi && !wifiSsid.trim()) ||
                  ct.provisioning ||
                  (brokerSource === 'custom' && customEnable && !customHost.trim())
                }
              >
                <ChevronRight className="mr-2 h-4 w-4" />
                {t('devices:ble.apply')}
              </Button>
            </div>
            {/* Scan wifi_init is already explained in the Wi‑Fi card — don't duplicate in footer. */}
            {(mqttFormError || cellFormError || webhookFormError || ct.error) && (
              <p className="text-center text-sm text-error">
                {mqttFormError ||
                  cellFormError ||
                  webhookFormError ||
                  (ct.error ? provErrorCopy(t, ct.error) : null)}
              </p>
            )}
          </div>
        </StepActions>
      )}
    </div>
  )
}

function InfoRow({ label, value }: { label: string; value: string }) {
  return (
    <div className="flex items-start justify-between gap-3">
      <span className="shrink-0 text-muted-foreground">{label}</span>
      <span className="break-all text-right font-medium">{value}</span>
    </div>
  )
}

function CapsChecklistItem({
  icon: Icon,
  title,
  hint,
}: {
  icon: typeof Wifi
  title: string
  hint: string
}) {
  return (
    <li className="flex gap-3">
      <div className="flex size-8 shrink-0 items-center justify-center rounded-lg bg-card ring-1 ring-inset ring-border">
        <Icon className="size-4 text-primary" aria-hidden />
      </div>
      <div className="min-w-0 space-y-0.5 pt-0.5">
        <p className="text-sm font-medium">{title}</p>
        <p className="text-xs leading-relaxed text-muted-foreground">{hint}</p>
      </div>
    </li>
  )
}

type CapsT = (key: string, opts?: Record<string, string>) => string

function humanizeSectionName(name: string, t: CapsT): string {
  switch (name) {
    case 'wifi':
      return t('devices:ctProv.capsItemWifi')
    case 'halow':
      return t('devices:ctProv.capsItemHalow')
    case 'cellular':
      return t('devices:ctProv.capsItemCellular')
    case 'mqtt':
      return t('devices:ctProv.capsItemMqtt')
    case 'webhook':
      return t('devices:ctProv.capsItemWebhook')
    case 'ethernet':
      return t('devices:ctProv.sectionEthernet')
    case 'comm':
      return t('devices:ctProv.sectionComm')
    default:
      return name
  }
}

function humanizeNetworkName(network: string, t: CapsT): string {
  switch (network) {
    case 'wifi':
      return t('devices:ctProv.networkWifi')
    case 'halow':
      return t('devices:ctProv.networkHalow')
    case 'cellular':
      return t('devices:ctProv.networkCellular')
    case 'ethernet':
      return t('devices:ctProv.networkEthernet')
    default:
      return network
  }
}
