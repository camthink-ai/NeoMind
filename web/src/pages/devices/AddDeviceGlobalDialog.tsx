import { useState, useEffect, useCallback, ReactNode } from 'react'
import { useTranslation } from 'react-i18next'
import {
  Bluetooth, Pencil, Search, RefreshCw, Cpu, Plus,
  Wifi, Globe, Radio,
  CheckCircle2, XCircle, Loader2,
  Copy, LucideIcon, KeyRound, Download, Lock, ShieldCheck,
} from 'lucide-react'
import { cn } from '@/lib/utils'
import { textMini } from "@/design-system/tokens/typography"
import { useIsMobile } from '@/hooks/useMobile'

import {
  FullScreenDialog,
  FullScreenDialogHeader,
  FullScreenDialogContent,
  FullScreenDialogMain,
  FullScreenDialogSidebar,
  FullScreenDialogFooter,
} from '@/components/automation/dialog/FullScreenDialog'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import { FormField } from '@/components/ui/field'
import { FormSection, FormSectionGroup } from '@/components/ui/form-section'
import { BleProvisionTab } from './BleProvisionTab'
import { toast } from '@/components/ui/use-toast'
import { api } from '@/lib/api'
import { useServerUrl } from '@/lib/server-url'
import { useStore } from '@/store'
import { fetchCache } from '@/lib/utils/async'
import type { DeviceType, AddDeviceRequest, ConnectionConfig, MqttStatus, ExternalBroker } from '@/types'
import { copyToClipboard } from '@/lib/clipboard'

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

interface AddDeviceGlobalDialogProps {
  open: boolean
  onOpenChange: (open: boolean) => void
  deviceTypes: DeviceType[]
  onAdd: (request: AddDeviceRequest) => Promise<boolean>
  adding: boolean
}

type TabValue = 'ble' | 'manual' | 'auto'

interface TabItem {
  value: TabValue
  icon: LucideIcon
  labelKey: string
  descKey: string
}

// ---------------------------------------------------------------------------
// Tab definitions
// ---------------------------------------------------------------------------

const TABS: TabItem[] = [
  { value: 'ble', icon: Bluetooth, labelKey: 'devices:ble.tabTitle', descKey: 'devices:ble.tabDesc' },
  { value: 'manual', icon: Pencil, labelKey: 'devices:add.title', descKey: 'devices:add.tabDesc' },
  { value: 'auto', icon: Search, labelKey: 'devices:auto.title', descKey: 'devices:auto.tabDesc' },
]

function generateRandomId(): string {
  const chars = 'abcdefghijklmnopqrstuvwxyz0123456789'
  let result = ''
  for (let i = 0; i < 10; i++) {
    result += chars.charAt(Math.floor(Math.random() * chars.length))
  }
  return result
}

function generateWebhookToken(): string {
  const chars = 'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789'
  let result = 'whk_'
  for (let i = 0; i < 32; i++) {
    result += chars.charAt(Math.floor(Math.random() * chars.length))
  }
  return result
}

// ---------------------------------------------------------------------------
// Main Component
// ---------------------------------------------------------------------------

export function AddDeviceGlobalDialog({
  open,
  onOpenChange,
  deviceTypes,
  onAdd,
  adding,
}: AddDeviceGlobalDialogProps) {
  const { t } = useTranslation(['common', 'devices'])
  const isMobile = useIsMobile()
  const fetchDevices = useStore((s) => s.fetchDevices)
  const fetchDeviceTypes = useStore((s) => s.fetchDeviceTypes)
  const [activeTab, setActiveTab] = useState<TabValue>('ble')

  // Footer content supplied by the active tab
  const [footer, setFooter] = useState<ReactNode>(null)
  const setTabFooter = useCallback((node: ReactNode) => setFooter(node), [])

  // Ensure device types are loaded when dialog opens
  useEffect(() => {
    if (open) fetchDeviceTypes()
  }, [open, fetchDeviceTypes])

  // Reset footer when switching tabs (old tab unmounts, new tab hasn't set its footer yet)
  useEffect(() => {
    setFooter(null)
  }, [activeTab])

  return (
    <FullScreenDialog open={open} onOpenChange={onOpenChange}>
      <FullScreenDialogHeader
        icon={<Cpu className="w-5 h-5" />}
        iconBg="bg-info-light"
        iconColor="text-info"
        title={t('devices:addDevice')}
        subtitle={t('devices:globalDialog.subtitle')}
        onClose={() => onOpenChange(false)}
      />

      <FullScreenDialogContent>
        {/* Sidebar: tab navigation (hidden on mobile) */}
        <FullScreenDialogSidebar>
          <nav className="flex flex-col py-2 gap-0.5 px-2">
            {TABS.map((tab) => {
              const Icon = tab.icon
              const isActive = activeTab === tab.value
              return (
                <button
                  key={tab.value}
                  onClick={() => setActiveTab(tab.value)}
                  className={cn(
                    'flex items-center gap-2.5 px-3 py-3 rounded-lg text-sm font-medium transition-all text-left',
                    isActive
                      ? 'bg-card shadow-sm border border-primary-light text-primary'
                      : 'text-muted-foreground hover:text-foreground hover:bg-muted-30 border border-transparent'
                  )}
                >
                  <div className={cn(
                    'w-8 h-8 rounded-lg flex items-center justify-center shrink-0',
                    isActive ? 'bg-primary text-primary-foreground' : 'bg-muted-30'
                  )}>
                    <Icon className="h-4 w-4" />
                  </div>
                  <div className="min-w-0">
                    <span className="truncate block">{t(tab.labelKey)}</span>
                    <span className={cn(textMini, 'leading-tight block mt-0.5 truncate text-muted-foreground')}>
                      {t(tab.descKey)}
                    </span>
                  </div>
                </button>
              )
            })}
          </nav>
        </FullScreenDialogSidebar>

        {/* Center column: main + footer stacked vertically */}
        <div className="flex-1 flex flex-col overflow-hidden">
          {/* Main scrollable content */}
          <FullScreenDialogMain>
            <div className={cn("max-w-2xl mx-auto py-6", isMobile ? "px-3" : "px-5 md:px-8")}>
              {/* Mobile tab bar - grid layout for equal width */}
              {isMobile && (
                <div className="grid grid-cols-[minmax(0,1fr)_minmax(0,1fr)_minmax(0,1fr)] gap-2 mb-5">
                  {TABS.map((tab) => {
                    const Icon = tab.icon
                    const isActive = activeTab === tab.value
                    return (
                      <button
                        key={tab.value}
                        onClick={() => setActiveTab(tab.value)}
                        className={cn(
                          'flex flex-col items-center gap-1.5 py-3 rounded-lg text-xs font-medium transition-all',
                          isActive
                            ? 'bg-primary text-primary-foreground'
                            : 'bg-muted-30 text-muted-foreground active:bg-muted'
                        )}
                      >
                        <Icon className="h-5 w-5" />
                        <span className="leading-tight">{t(tab.labelKey)}</span>
                      </button>
                    )
                  })}
                </div>
              )}

              {/* Tab content */}
              {activeTab === 'ble' && open && (
                <BleProvisionTab
                  onComplete={() => {
                    fetchCache.invalidate('devices')
                    fetchDevices()
                    onOpenChange(false)
                  }}
                />
              )}

              {activeTab === 'manual' && open && (
                <ManualAddForm
                  deviceTypes={deviceTypes}
                  onAdd={onAdd}
                  adding={adding}
                  onSuccess={() => onOpenChange(false)}
                  renderFooter={setTabFooter}
                />
              )}

              {activeTab === 'auto' && open && (
                <AutoDiscoveryTab renderFooter={setTabFooter} />
              )}
            </div>
          </FullScreenDialogMain>

          {/* Footer: action buttons from the active tab */}
          {footer && <FullScreenDialogFooter>{footer}</FullScreenDialogFooter>}
        </div>
      </FullScreenDialogContent>
    </FullScreenDialog>
  )
}

// ---------------------------------------------------------------------------
// Auto Discovery Tab — Rich guided experience
// ---------------------------------------------------------------------------

interface StepCardProps {
  step: number
  icon: LucideIcon
  title: string
  description: string
  status?: 'done' | 'active' | 'pending'
}

function StepCard({ step, icon: Icon, title, description, status = 'pending' }: StepCardProps) {
  return (
    <div className={cn(
      'relative flex gap-4 p-4 rounded-xl border transition-colors',
      status === 'done' && 'border-success bg-success-light',
      status === 'active' && 'border-primary bg-card',
      status === 'pending' && 'border-border bg-card',
    )}>
      {/* Step number */}
      <div className={cn(
        'w-8 h-8 rounded-full flex items-center justify-center text-sm font-bold shrink-0',
        status === 'done' && 'bg-success text-primary-foreground',
        status === 'active' && 'bg-primary text-primary-foreground',
        status === 'pending' && 'bg-muted-30 text-muted-foreground',
      )}>
        {status === 'done' ? <CheckCircle2 className="h-4 w-4" /> : step}
      </div>
      <div className="flex-1 min-w-0 space-y-1">
        <div className="flex items-center gap-2">
          <Icon className="h-4 w-4 text-muted-foreground shrink-0" />
          <h4 className="text-sm font-medium">{title}</h4>
        </div>
        <p className="text-xs text-muted-foreground leading-relaxed">{description}</p>
      </div>
    </div>
  )
}

function AutoDiscoveryTab({ renderFooter }: { renderFooter: (node: ReactNode) => void }) {
  const { t } = useTranslation(['devices', 'common'])
  const isMobile = useIsMobile()
  const serverUrl = useServerUrl()
  const [mqttStatus, setMqttStatus] = useState<MqttStatus | null>(null)
  const [brokers, setBrokers] = useState<ExternalBroker[]>([])
  const [embeddedConfig, setEmbeddedConfig] = useState<{
    auth_enabled: boolean
    tls_enabled: boolean
    tls_ca_path: string | null
    credentials: { username: string; password: string }[]
  } | null>(null)
  const [loading, setLoading] = useState(true)
  const [activeMethod, setActiveMethod] = useState<'mqtt' | 'webhook'>('mqtt')

  useEffect(() => {
    renderFooter(null)
  }, [renderFooter])

  // Fetch MQTT status + full broker details + embedded config
  useEffect(() => {
    Promise.all([
      api.getMqttStatus().catch(() => null),
      api.getBrokers().catch(() => null),
      api.getEmbeddedBrokerConfig().catch(() => null),
    ]).then(([statusRes, brokersRes, embeddedRes]) => {
      setMqttStatus(statusRes?.status ?? null)
      setBrokers(brokersRes?.brokers ?? [])
      if (embeddedRes) {
        setEmbeddedConfig({
          auth_enabled: embeddedRes.auth_enabled,
          tls_enabled: embeddedRes.tls_enabled,
          tls_ca_path: embeddedRes.tls_ca_path,
          credentials: embeddedRes.credentials || [],
        })
      }
    }).finally(() => setLoading(false))
  }, [])

  const brokerRunning = mqttStatus?.connected ?? false
  const serverIp = mqttStatus?.server_ip || serverUrl.replace(/^https?:\/\//, '')
  const brokerPort = mqttStatus?.listen_port ?? 1883
  // Use canonical serverUrl (LAN IP in Tauri, window.location.origin in browser)
  // Don't use serverIp:apiPort as it may not be accessible from external networks
  const webhookBaseUrl = `${serverUrl}/api/devices/{device_id}/webhook`
  const enabledBrokers = brokers.filter(b => b.enabled)
  const hasAnyBroker = brokerRunning || enabledBrokers.length > 0

  // Determine step statuses
  const step1Status: StepCardProps['status'] = hasAnyBroker ? 'done' : 'active'
  const step2Status: StepCardProps['status'] = hasAnyBroker ? 'active' : 'pending'
  const step3Status: StepCardProps['status'] = hasAnyBroker ? 'active' : 'pending'

  if (loading) {
    return (
      <div className="flex items-center justify-center py-12">
        <Loader2 className="h-6 w-6 animate-spin text-muted-foreground" />
      </div>
    )
  }

  return (
    <FormSectionGroup>
      {/* How it works — Step cards */}
      <FormSection
        title={t('devices:auto.howItWorks')}
        description={t('devices:auto.howItWorksDesc')}
      >
        <div className="space-y-3">
          <StepCard
            step={1}
            icon={Radio}
            title={t('devices:auto.step1_title')}
            description={t('devices:auto.step1_desc')}
            status={step1Status}
          />
          <StepCard
            step={2}
            icon={Wifi}
            title={t('devices:auto.step2_title')}
            description={t('devices:auto.step2_desc')}
            status={step2Status}
          />
          <StepCard
            step={3}
            icon={Search}
            title={t('devices:auto.step3_title')}
            description={t('devices:auto.step3_desc')}
            status={step3Status}
          />
        </div>
      </FormSection>

      {/* Connection methods — MQTT & Webhook */}
      <FormSection
        title={t('devices:auto.brokerInfo')}
        description={t('devices:auto.brokerInfoDesc')}
      >
        {/* Method toggle */}
        <div className="flex gap-2 mb-4">
          {([
            { key: 'mqtt' as const, icon: Radio, color: 'text-accent-orange', bg: 'bg-accent-orange-light' },
            { key: 'webhook' as const, icon: Globe, color: 'text-accent-purple', bg: 'bg-accent-purple-light' },
          ]).map(({ key, icon: MethodIcon, color, bg }) => (
            <button
              key={key}
              type="button"
              onClick={() => setActiveMethod(key)}
              className={cn(
                'flex items-center gap-2 px-4 py-2 rounded-lg text-sm font-medium transition-all',
                activeMethod === key
                  ? 'bg-card border border-primary shadow-sm'
                  : 'bg-muted-30 text-muted-foreground hover:bg-card'
              )}
            >
              <div className={cn('w-6 h-6 rounded flex items-center justify-center', bg)}>
                <MethodIcon className={cn('h-3.5 w-3.5', color)} />
              </div>
              {t(`devices:auto.method${key.charAt(0).toUpperCase() + key.slice(1)}`)}
            </button>
          ))}
        </div>

        {/* MQTT panel — built-in + all external brokers */}
        {activeMethod === 'mqtt' && (
          <div className="space-y-3">
            {/* Built-in broker */}
            <div className="rounded-xl border bg-card overflow-hidden">
              <div className={cn(
                'flex items-center gap-2 px-4 py-2.5 text-sm font-medium',
                brokerRunning
                  ? 'bg-success-light text-success'
                  : 'bg-error-light text-error'
              )}>
                {brokerRunning ? (
                  <CheckCircle2 className="h-4 w-4" />
                ) : (
                  <XCircle className="h-4 w-4" />
                )}
                {t('devices:auto.builtInBroker')}
                <span className="ml-auto">{brokerRunning ? t('devices:auto.brokerRunning') : t('devices:auto.brokerStopped')}</span>
              </div>
              <div className="p-4 space-y-3">
                <div className={cn("gap-3", isMobile ? "grid grid-cols-1" : "grid grid-cols-2")}>
                  <div className="space-y-1">
                    <span className="text-xs text-muted-foreground">{t('devices:auto.protocol')}</span>
                    <code className="block text-sm font-mono bg-muted-30 rounded-md px-3 py-1.5">
                      {embeddedConfig?.tls_enabled ? 'mqtts://' : 'mqtt://'}{serverIp}:{brokerPort}
                    </code>
                  </div>
                  <div className="space-y-1">
                    <span className="text-xs text-muted-foreground">{t('devices:auto.brokerAddress')}</span>
                    <code className="block text-sm font-mono bg-muted-30 rounded-md px-3 py-1.5">
                      {serverIp}:{brokerPort}
                    </code>
                  </div>
                </div>
                {/* TLS & Auth status badges */}
                <div className="flex flex-wrap gap-2">
                  {embeddedConfig?.tls_enabled && (
                    <div className="flex items-center gap-1.5 px-2.5 py-1 rounded-md bg-success-light text-success text-xs font-medium">
                      <Lock className="h-3 w-3" />
                      {t('devices:auto.tlsEnabled')}
                    </div>
                  )}
                  {embeddedConfig?.auth_enabled ? (
                    <div className="flex items-center gap-1.5 px-2.5 py-1 rounded-md bg-warning-light text-warning text-xs font-medium">
                      <ShieldCheck className="h-3 w-3" />
                      {t('devices:auto.authRequired')}
                    </div>
                  ) : (
                    <div className="flex items-center gap-1.5 px-2.5 py-1 rounded-md bg-muted-30 text-muted-foreground text-xs font-medium">
                      {t('devices:auto.authNotRequired')}
                    </div>
                  )}
                </div>
                {/* CA cert download */}
                {embeddedConfig?.tls_enabled && embeddedConfig.tls_ca_path && (
                  <Button
                    variant="outline"
                    size="sm"
                    className="w-full"
                    onClick={async () => {
                      try {
                        await api.downloadMqttCaCert()
                        toast({ title: t('devices:auto.caCertDownloaded') })
                      } catch { /* ignore */ }
                    }}
                  >
                    <Download className="h-4 w-4 mr-2" />
                    {t('devices:auto.caCertDownload')}
                  </Button>
                )}
                {/* Credentials */}
                {embeddedConfig?.auth_enabled && embeddedConfig.credentials.length > 0 && (
                  <div className="space-y-1">
                    <span className="text-xs text-muted-foreground">{t('devices:auto.authInfo')}</span>
                    {embeddedConfig.credentials.map((cred) => (
                      <div key={cred.username} className="flex items-center gap-2">
                        <code className="flex-1 text-sm font-mono bg-muted-30 rounded-md px-3 py-1.5">
                          {cred.username} / {cred.password}
                        </code>
                        <Button
                          variant="ghost"
                          size="icon"
                          className="shrink-0 h-7 w-7"
                          onClick={() => {
                            copyToClipboard(`${cred.username}:${cred.password}`)
                            toast({ title: t('common:copied') })
                          }}
                        >
                          <Copy className="h-3.5 w-3.5" />
                        </Button>
                      </div>
                    ))}
                  </div>
                )}
              </div>
            </div>

            {/* External brokers — full details from /api/brokers */}
            {enabledBrokers.map((broker) => (
              <div key={broker.id} className="rounded-xl border bg-card overflow-hidden">
                <div className={cn(
                  'flex items-center gap-2 px-4 py-2.5 text-sm font-medium',
                  broker.connected
                    ? 'bg-success-light text-success'
                    : 'bg-error-light text-error'
                )}>
                  {broker.connected ? (
                    <CheckCircle2 className="h-4 w-4" />
                  ) : (
                    <XCircle className="h-4 w-4" />
                  )}
                  <span className="truncate">{broker.name}</span>
                  <span className="ml-auto text-xs">{t('devices:auto.externalBroker')}</span>
                  {broker.tls && (
                    <span className="text-xs bg-muted-30 px-1.5 py-0.5 rounded">{t('devices:auto.tlsEnabled')}</span>
                  )}
                </div>
                <div className="p-4 space-y-3">
                  <div className={cn("gap-3", isMobile ? "grid grid-cols-1" : "grid grid-cols-2")}>
                    <div className="space-y-1">
                      <span className="text-xs text-muted-foreground">{t('devices:auto.brokerAddress')}</span>
                      <code className="block text-sm font-mono bg-muted-30 rounded-md px-3 py-1.5 break-all">
                        {broker.broker}
                      </code>
                    </div>
                    <div className="space-y-1">
                      <span className="text-xs text-muted-foreground">{t('devices:auto.brokerPort')}</span>
                      <code className="block text-sm font-mono bg-muted-30 rounded-md px-3 py-1.5">
                        {broker.port}
                      </code>
                    </div>
                  </div>
                  {/* Auth info */}
                  {broker.username && (
                    <div className="space-y-1">
                      <span className="text-xs text-muted-foreground">{t('devices:auto.authInfo')}</span>
                      <div className="flex items-center gap-2">
                        <code className="flex-1 text-sm font-mono bg-muted-30 rounded-md px-3 py-1.5">
                          {broker.username} / ••••••••
                        </code>
                        <Button
                          variant="ghost"
                          size="icon"
                          className="shrink-0 h-7 w-7"
                          onClick={() => {
                            broker.username && copyToClipboard(broker.username)
                            toast({ title: t('common:copied') })
                          }}
                        >
                          <Copy className="h-3.5 w-3.5" />
                        </Button>
                      </div>
                      <span className="text-xs text-muted-foreground">{t('devices:auto.passwordHidden')}</span>
                    </div>
                  )}
                  {/* Subscribe topics */}
                  {broker.subscribe_topics && broker.subscribe_topics.length > 0 && (
                    <div className="space-y-1">
                      <span className="text-xs text-muted-foreground">{t('devices:auto.subscribeTopics')}</span>
                      <div className="space-y-1">
                        {broker.subscribe_topics.map((topic, i) => (
                          <div key={i} className="flex items-center gap-2">
                            <code className="flex-1 text-sm font-mono bg-muted-30 rounded-md px-3 py-1.5 break-all">
                              {topic}
                            </code>
                            <Button
                              variant="ghost"
                              size="icon"
                              className="shrink-0 h-7 w-7"
                              onClick={() => {
                                copyToClipboard(topic)
                                toast({ title: t('common:copied') })
                              }}
                            >
                              <Copy className="h-3.5 w-3.5" />
                            </Button>
                          </div>
                        ))}
                      </div>
                    </div>
                  )}
                </div>
              </div>
            ))}
          </div>
        )}

        {/* Webhook panel */}
        {activeMethod === 'webhook' && (
          <div className="rounded-xl border bg-card p-4 space-y-3">
            <p className="text-sm text-muted-foreground">
              {t('devices:auto.webhookEndpointDesc')}
            </p>
            <div className="space-y-1">
              <span className="text-xs text-muted-foreground">{t('devices:auto.webhookEndpoint')}</span>
              <code className="block text-sm font-mono bg-muted-30 rounded-md px-3 py-2 break-all">
                {webhookBaseUrl}
              </code>
            </div>
            <p className="xs text-muted-foreground">
              {t('devices:auto.webhookUrlHint')}
            </p>
            <div className="space-y-1">
              <div className="flex items-center justify-between">
                <span className="text-xs text-muted-foreground">{t('devices:auto.webhookCurlTitle')}</span>
                <Button
                  variant="ghost"
                  size="icon"
                  className="h-7 w-7 shrink-0"
                  onClick={() => {
                    const cmd = `curl -X POST ${serverUrl}/api/devices/my-sensor-001/webhook \\\n  -H "Content-Type: application/json" \\\n  -d '{\n    "data": {\n      "temperature": 23.5,\n      "humidity": 65.2\n    }\n  }'`
                    copyToClipboard(cmd)
                    toast({ title: t('devices:auto.webhookCurlCopied') })
                  }}
                >
                  <Copy className="h-3.5 w-3.5" />
                </Button>
              </div>
              <pre className="text-xs font-mono bg-muted-30 rounded-md px-3 py-2 overflow-x-auto whitespace-pre leading-relaxed">{`curl -X POST ${serverUrl}/api/devices/my-sensor-001/webhook \\
  -H "Content-Type: application/json" \\
  -d '{
    "data": {
      "temperature": 23.5,
      "humidity": 65.2
    }
  }'`}</pre>
            </div>
          </div>
        )}
      </FormSection>

      {/* Tips */}
      <FormSection title={t('devices:auto.tips')}>
        <div className="space-y-2">
          {[1, 2, 3].map((n) => (
            <div key={n} className="flex items-start gap-2 text-sm text-muted-foreground">
              <span className="text-xs leading-5 mt-px">&#8226;</span>
              <span>{t(`devices:auto.tip${n}`)}</span>
            </div>
          ))}
        </div>
      </FormSection>
    </FormSectionGroup>
  )
}

// ---------------------------------------------------------------------------
// Manual Add Form — Improved with adapter type cards
// ---------------------------------------------------------------------------

const ADAPTER_TYPES = [
  { value: 'mqtt' as const, icon: Radio, color: 'text-accent-orange', bg: 'bg-accent-orange-light' },
  { value: 'webhook' as const, icon: Wifi, color: 'text-accent-purple', bg: 'bg-accent-purple-light' },
]

function ManualAddForm({
  deviceTypes,
  onAdd,
  adding,
  onSuccess,
  renderFooter,
}: {
  deviceTypes: DeviceType[]
  onAdd: (request: AddDeviceRequest) => Promise<boolean>
  adding: boolean
  onSuccess: () => void
  renderFooter: (node: ReactNode) => void
}) {
  const { t } = useTranslation(['common', 'devices'])
  const isMobile = useIsMobile()
  const serverUrl = useServerUrl()

  const [selectedDeviceType, setSelectedDeviceType] = useState('')
  const [deviceId, setDeviceId] = useState('')
  const [deviceName, setDeviceName] = useState('')
  const [adapterType, setAdapterType] = useState<'mqtt' | 'webhook'>('mqtt')
  const [connectionConfig, setConnectionConfig] = useState<ConnectionConfig>({})
  const [errors, setErrors] = useState<Record<string, string>>({})
  const [serverIp, setServerIp] = useState('')
  const [webhookToken, setWebhookToken] = useState('')

  // Broker connection guide state
  const [selectedBrokerId, setSelectedBrokerId] = useState<string>('embedded')
  const [mqttStatus, setMqttStatus] = useState<MqttStatus | null>(null)
  const [embeddedConfig, setEmbeddedConfig] = useState<{
    listen: string
    port: number
    auth_enabled: boolean
    tls_enabled: boolean
    tls_ca_path: string | null
    credentials: { username: string; password: string }[]
  } | null>(null)
  const [externalBrokers, setExternalBrokers] = useState<ExternalBroker[]>([])

  // Fetch server IP + broker data
  useEffect(() => {
    Promise.all([
      api.getMqttStatus().catch(() => null),
      api.getEmbeddedBrokerConfig().catch(() => null),
      api.getBrokers().catch(() => null),
    ]).then(([statusRes, embeddedRes, brokersRes]) => {
      if (statusRes?.status) {
        setMqttStatus(statusRes.status)
        setServerIp(statusRes.status.server_ip)
      }
      if (embeddedRes) {
        setEmbeddedConfig({
          listen: embeddedRes.listen,
          port: embeddedRes.port,
          auth_enabled: embeddedRes.auth_enabled,
          tls_enabled: embeddedRes.tls_enabled,
          tls_ca_path: embeddedRes.tls_ca_path,
          credentials: embeddedRes.credentials || [],
        })
      }
      const enabled = (brokersRes?.brokers ?? []).filter((b: ExternalBroker) => b.enabled)
      setExternalBrokers(enabled)
    })
  }, [])

  useEffect(() => {
    if (!deviceId) setDeviceId(generateRandomId())
  }, [])

  useEffect(() => {
    if (adapterType === 'mqtt' && selectedDeviceType && deviceId) {
      setConnectionConfig({
        telemetry_topic: `device/${selectedDeviceType}/${deviceId}/uplink`,
        command_topic: `device/${selectedDeviceType}/${deviceId}/downlink`,
      })
    } else {
      setConnectionConfig({})
    }
    if (adapterType !== 'webhook') {
      setWebhookToken('')
    }
  }, [adapterType, selectedDeviceType, deviceId])

  const handleSubmit = useCallback(async () => {
    const newErrors: Record<string, string> = {}
    if (!selectedDeviceType) {
      newErrors.deviceType = t('devices:deviceType') + ' is required'
    }
    setErrors(newErrors)
    if (Object.keys(newErrors).length > 0) return

    const request: AddDeviceRequest = {
      device_id: deviceId || undefined,
      name: deviceName || deviceId || selectedDeviceType,
      device_type: selectedDeviceType,
      adapter_type: adapterType,
      connection_config: {
        ...connectionConfig,
        ...(adapterType === 'webhook' && webhookToken ? { webhook_token: webhookToken } : {}),
      },
    }

    const success = await onAdd(request)
    if (success) {
      toast({
        title: t('devices:add.success'),
        description: deviceId ? t('devices:add.successWithId', { deviceId }) : t('devices:add.successGeneric'),
      })
      onSuccess()
    } else {
      toast({
        title: t('devices:add.error'),
        description: t('devices:add.retryMessage'),
        variant: 'destructive',
      })
    }
  }, [selectedDeviceType, adapterType, connectionConfig, deviceId, deviceName, onAdd, onSuccess, t])

  // Push footer button to parent
  useEffect(() => {
    renderFooter(
      <Button onClick={handleSubmit} disabled={!selectedDeviceType || adding}>
        {adding ? t('common:loading') : t('common:add')}
      </Button>
    )
  }, [selectedDeviceType, adding, handleSubmit, renderFooter, t])

  return (
    <FormSectionGroup>
      {/* Basic Info */}
      <FormSection>
        {/* Device Type */}
        <FormField label={t('devices:deviceType')} required error={errors.deviceType}>
          <Select value={selectedDeviceType} onValueChange={(v) => { setSelectedDeviceType(v); setErrors(prev => { const next = { ...prev }; delete next.deviceType; return next }) }}>
            <SelectTrigger>
              <SelectValue placeholder={t('devices:add.typePlaceholder')} />
            </SelectTrigger>
            <SelectContent>
              {deviceTypes.map((type) => (
                <SelectItem key={type.device_type} value={type.device_type}>
                  {type.name}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </FormField>

        {/* Device ID & Name */}
        <div className="grid grid-cols-1 sm:grid-cols-2 gap-4">
          <FormField label={t('devices:deviceId')}>
            <div className="flex gap-2">
              <Input
                value={deviceId}
                onChange={(e) => setDeviceId(e.target.value)}
                placeholder={t('devices:id.autoGenerate')}
                className="font-mono"
              />
              <Button
                type="button"
                variant="outline"
                size="icon"
                onClick={() => setDeviceId(generateRandomId())}
                title={t('devices:id.regenerate')}
              >
                <RefreshCw className="h-4 w-4" />
              </Button>
            </div>
          </FormField>
          <FormField label={t('devices:deviceName')}>
            <Input
              value={deviceName}
              onChange={(e) => setDeviceName(e.target.value)}
              placeholder={t('common:optional')}
            />
          </FormField>
        </div>
      </FormSection>

      {/* Connection Settings Section */}
      <FormSection
        title={t('devices:add.connectionSettings')}
        description={t('devices:add.connectionSettingsDesc')}
      >
        {/* Adapter type — visual cards */}
        <div className={cn("gap-2", isMobile ? "grid grid-cols-[minmax(0,1fr)_minmax(0,1fr)]" : "grid grid-cols-[minmax(0,1fr)_minmax(0,1fr)]")}>
          {ADAPTER_TYPES.map(({ value, icon: AdapterIcon, color, bg }) => {
            const isActive = adapterType === value
            return (
              <button
                key={value}
                type="button"
                onClick={() => setAdapterType(value)}
                className={cn(
                  'flex flex-col items-center gap-2 p-3 rounded-xl border-2 transition-all text-center',
                  isActive
                    ? 'border-primary bg-card'
                    : 'border-transparent bg-muted-30 hover:bg-card hover:border-border'
                )}
              >
                <div className={cn(
                  'w-9 h-9 rounded-lg flex items-center justify-center',
                  bg
                )}>
                  <AdapterIcon className={cn('h-4 w-4', color)} />
                </div>
                <div className="min-w-0">
                  <span className={cn(
                    'text-sm font-medium block',
                    isActive ? 'text-foreground' : 'text-muted-foreground'
                  )}>
                    {t(`devices:add.adapterLabel_${value}`)}
                  </span>
                  <span className={cn(textMini, "text-muted-foreground leading-tight block mt-0.5 truncate max-w-[140px]")}>
                    {t(`devices:add.adapterDesc_${value}`)}
                  </span>
                </div>
              </button>
            )
          })}
        </div>

        {/* MQTT Config */}
        {adapterType === 'mqtt' && (
          <div className="space-y-3 pt-1">
            {/* Broker connection guide */}
            <div className="rounded-lg border bg-muted-30 p-4 space-y-3">
              <div className="flex items-center gap-2 text-sm font-medium">
                <Radio className="h-4 w-4 text-accent-orange" />
                {t('devices:add.connectionGuide')}
              </div>
              <p className="text-xs text-muted-foreground">{t('devices:add.connectionGuideDesc')}</p>

              {/* Broker selector */}
              <FormField label={t('devices:add.selectBroker')}>
                <Select value={selectedBrokerId} onValueChange={setSelectedBrokerId}>
                  <SelectTrigger>
                    <SelectValue placeholder={t('devices:add.selectBrokerPlaceholder')} />
                  </SelectTrigger>
                  <SelectContent>
                    {/* Embedded broker option */}
                    <SelectItem value="embedded" disabled={!mqttStatus?.connected}>
                      <span className="flex items-center gap-2">
                        {mqttStatus?.connected ? (
                          <CheckCircle2 className="h-3 w-3 text-success" />
                        ) : (
                          <XCircle className="h-3 w-3 text-error" />
                        )}
                        {t('devices:auto.builtInBroker')}
                      </span>
                    </SelectItem>
                    {/* External brokers */}
                    {externalBrokers.map((b) => (
                      <SelectItem key={b.id} value={b.id}>
                        <span className="flex items-center gap-2">
                          {b.connected ? (
                            <CheckCircle2 className="h-3 w-3 text-success" />
                          ) : (
                            <XCircle className="h-3 w-3 text-error" />
                          )}
                          {b.name}
                        </span>
                      </SelectItem>
                    ))}
                  </SelectContent>
                </Select>
              </FormField>

              {/* Connection details card */}
              {selectedBrokerId === 'embedded' && embeddedConfig && (
                <div className="space-y-2">
                  <div className={cn("gap-3", isMobile ? "grid grid-cols-1" : "grid grid-cols-2")}>
                    <div className="space-y-1">
                      <span className="text-xs text-muted-foreground">{t('devices:add.protocol')}</span>
                      <code className="block text-sm font-mono bg-muted-30 rounded-md px-3 py-1.5">
                        {embeddedConfig.tls_enabled ? 'mqtts' : 'mqtt'}
                      </code>
                    </div>
                    <div className="space-y-1">
                      <span className="text-xs text-muted-foreground">{t('devices:add.serverAddress')}</span>
                      <code className="block text-sm font-mono bg-muted-30 rounded-md px-3 py-1.5">
                        {serverIp || mqttStatus?.server_ip || 'localhost'}:{embeddedConfig.port}
                      </code>
                    </div>
                  </div>
                  <div className="flex flex-wrap gap-2">
                    {embeddedConfig.tls_enabled && (
                      <div className="flex items-center gap-1.5 px-2.5 py-1 rounded-md bg-success-light text-success text-xs font-medium">
                        <Lock className="h-3 w-3" />
                        TLS: {embeddedConfig.tls_ca_path ? t('devices:add.tlsWithCa') : t('devices:add.tlsNoCa')}
                      </div>
                    )}
                    {embeddedConfig.auth_enabled ? (
                      <div className="flex items-center gap-1.5 px-2.5 py-1 rounded-md bg-warning-light text-warning text-xs font-medium">
                        <ShieldCheck className="h-3 w-3" />
                        {t('devices:add.authStatus')}: {t('devices:add.authRequired')}
                      </div>
                    ) : (
                      <div className="flex items-center gap-1.5 px-2.5 py-1 rounded-md bg-muted-30 text-muted-foreground text-xs font-medium">
                        {t('devices:add.authStatus')}: {t('devices:add.authNotRequired')}
                      </div>
                    )}
                  </div>
                  {embeddedConfig.tls_enabled && embeddedConfig.tls_ca_path && (
                    <Button
                      variant="outline"
                      size="sm"
                      className="w-full"
                      onClick={async () => {
                        try {
                          await api.downloadMqttCaCert()
                          toast({ title: t('devices:add.caCertDownloaded') })
                        } catch { /* ignore */ }
                      }}
                    >
                      <Download className="h-4 w-4 mr-2" />
                      {t('devices:add.caCertDownload')}
                    </Button>
                  )}
                  {/* Credentials */}
                  {embeddedConfig.auth_enabled && embeddedConfig.credentials.length > 0 && (
                    <div className="space-y-1">
                      <span className="text-xs text-muted-foreground">{t('devices:auto.authInfo')}</span>
                      {embeddedConfig.credentials.map((cred) => (
                        <div key={cred.username} className="flex items-center gap-2">
                          <code className="flex-1 text-sm font-mono bg-muted-30 rounded-md px-3 py-1.5">
                            {cred.username} / {cred.password}
                          </code>
                          <Button
                            variant="ghost"
                            size="icon"
                            className="shrink-0 h-7 w-7"
                            onClick={() => {
                              copyToClipboard(`${cred.username}:${cred.password}`)
                              toast({ title: t('common:copied') })
                            }}
                          >
                            <Copy className="h-3.5 w-3.5" />
                          </Button>
                        </div>
                      ))}
                    </div>
                  )}
                </div>
              )}

              {selectedBrokerId !== 'embedded' && (() => {
                const broker = externalBrokers.find(b => b.id === selectedBrokerId)
                if (!broker) return null
                return (
                  <div className="space-y-2">
                    <div className={cn("gap-3", isMobile ? "grid grid-cols-1" : "grid grid-cols-2")}>
                      <div className="space-y-1">
                        <span className="text-xs text-muted-foreground">{t('devices:add.protocol')}</span>
                        <code className="block text-sm font-mono bg-muted-30 rounded-md px-3 py-1.5">
                          {broker.tls ? 'mqtts' : 'mqtt'}
                        </code>
                      </div>
                      <div className="space-y-1">
                        <span className="text-xs text-muted-foreground">{t('devices:add.serverAddress')}</span>
                        <code className="block text-sm font-mono bg-muted-30 rounded-md px-3 py-1.5">
                          {broker.broker}:{broker.port}
                        </code>
                      </div>
                    </div>
                    <div className="flex flex-wrap gap-2">
                      {broker.tls && (
                        <div className="flex items-center gap-1.5 px-2.5 py-1 rounded-md bg-success-light text-success text-xs font-medium">
                          <Lock className="h-3 w-3" />
                          TLS: {t('devices:add.tlsEnabled')}
                        </div>
                      )}
                      {broker.username ? (
                        <div className="flex items-center gap-1.5 px-2.5 py-1 rounded-md bg-warning-light text-warning text-xs font-medium">
                          <ShieldCheck className="h-3 w-3" />
                          {t('devices:add.authStatus')}: {t('devices:add.authRequired')}
                        </div>
                      ) : (
                        <div className="flex items-center gap-1.5 px-2.5 py-1 rounded-md bg-muted-30 text-muted-foreground text-xs font-medium">
                          {t('devices:add.authStatus')}: {t('devices:add.authNotRequired')}
                        </div>
                      )}
                    </div>
                    {/* Credentials */}
                    {broker.username && (
                      <div className="space-y-1">
                        <span className="text-xs text-muted-foreground">{t('devices:auto.authInfo')}</span>
                        <div className="flex items-center gap-2">
                          <code className="flex-1 text-sm font-mono bg-muted-30 rounded-md px-3 py-1.5">
                            {broker.username}{broker.password ? ` / ${broker.password}` : ' / ••••••••'}
                          </code>
                          {broker.password && (
                            <Button
                              variant="ghost"
                              size="icon"
                              className="shrink-0 h-7 w-7"
                              onClick={() => {
                                copyToClipboard(`${broker.username}:${broker.password}`)
                                toast({ title: t('common:copied') })
                              }}
                            >
                              <Copy className="h-3.5 w-3.5" />
                            </Button>
                          )}
                        </div>
                      </div>
                    )}
                  </div>
                )
              })()}
            </div>

            <FormField label={t('devices:add.telemetryTopic')} helpText={t('devices:add.telemetryTopicHelp')}>
              <Input
                value={connectionConfig.telemetry_topic || ''}
                onChange={(e) => setConnectionConfig({ ...connectionConfig, telemetry_topic: e.target.value })}
                placeholder="device/{type}/{id}/uplink"
                className="font-mono text-sm"
              />
            </FormField>
            <FormField label={t('devices:add.commandTopic')} helpText={t('devices:add.commandTopicHelp')}>
              <Input
                value={connectionConfig.command_topic || ''}
                onChange={(e) => setConnectionConfig({ ...connectionConfig, command_topic: e.target.value })}
                placeholder="device/{type}/{id}/downlink"
                className="font-mono text-sm"
              />
            </FormField>
          </div>
        )}

        {/* Webhook Config */}
        {adapterType === 'webhook' && (
          <div className="space-y-3 pt-1">
            <div className="rounded-lg border bg-muted-30 p-4">
              <p className="text-sm text-muted-foreground mb-2">
                {t('devices:add.webhookUrlDescription')}
              </p>
              <code className="text-xs break-all block font-mono">
                {`${serverUrl}/api/devices/${deviceId}/webhook`}
              </code>
            </div>
            <FormField label={t('devices:add.webhookToken')}>
              <div className="flex gap-2">
                <Input
                  value={webhookToken}
                  onChange={(e) => setWebhookToken(e.target.value)}
                  placeholder={t('devices:add.webhookTokenPlaceholder')}
                  className="font-mono text-sm"
                />
                <Button
                  type="button"
                  variant="outline"
                  size="icon"
                  onClick={() => setWebhookToken(generateWebhookToken())}
                  title={t('devices:add.webhookTokenGenerate')}
                >
                  <KeyRound className="h-4 w-4" />
                </Button>
              </div>
              <p className="text-xs text-muted-foreground mt-1">
                {t('devices:add.webhookTokenDesc')}
              </p>
            </FormField>
          </div>
        )}

      </FormSection>
    </FormSectionGroup>
  )
}
