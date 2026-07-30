import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { useErrorHandler } from '@/hooks/useErrorHandler'
import { showErrorToast } from '@/lib/error-messages'
import {
  Server,
  Edit,
  Trash2,
  TestTube,
  Webhook,
  Radio,
  Copy,
  Check,
  Cpu,
  LucideIcon,
  Loader2,
  AlertTriangle,
  KeyRound,
  Settings,
} from 'lucide-react'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
  DialogContentBody,
} from '@/components/ui/dialog'
import { EmptyState, LoadingState, ListToolbar } from '@/components/shared'
import { cn } from '@/lib/utils'
import { api } from '@/lib/api'
import { useServerUrl } from '@/lib/server-url'
import { UniversalPluginConfigDialog, type PluginInstance, type UnifiedPluginType } from '@/components/plugins/UniversalPluginConfigDialog'
import { EmbeddedBrokerConfigDialog } from './EmbeddedBrokerConfigDialog'
import type { PluginConfigSchema, AdapterType } from '@/types'
import { useToast } from '@/hooks/use-toast'
import { ADAPTER_TYPES } from '@/constants/deviceAdapters'

// Icon mapping for adapter types
const ICON_MAP: Record<string, LucideIcon> = {
  Server,
  Webhook,
  Radio,
  Cpu,
}

type View = 'list' | 'detail'

// Config schemas for each adapter type
const getAdapterSchema = (adapterType: string): PluginConfigSchema => {
  switch (adapterType) {
    case 'mqtt':
      return {
        type: 'object',
        properties: {
          broker: {
            type: 'string',
            description: 'Broker address',
            format: 'hostname',
          },
          port: {
            type: 'number',
            description: 'Broker port',
            default: 1883,
            minimum: 1,
            maximum: 65535,
          },
          username: {
            type: 'string',
            description: 'Username for authentication',
          },
          password: {
            type: 'string',
            description: 'Password for authentication',
            secret: true,
          },
          tls: {
            type: 'boolean',
            description: 'Use TLS/SSL',
            default: false,
          },
          ca_cert: {
            type: 'string',
            description: 'CA certificate for TLS verification (PEM format). Required for self-signed certificates.',
            format: 'multiline',
          },
          client_cert: {
            type: 'string',
            description: 'Client certificate for mTLS authentication (PEM format).',
            format: 'multiline',
          },
          client_key: {
            type: 'string',
            description: 'Client private key for mTLS authentication (PEM format).',
            secret: true,
            format: 'multiline',
          },
          client_id: {
            type: 'string',
            description: 'Custom MQTT client ID. Leave empty to auto-generate.',
          },
          subscribe_topics: {
            type: 'array',
            description: 'Extra custom topics to subscribe (one per line). System topics (device/+/+/uplink, device/+/+/downlink) are always auto-subscribed. Wildcards: + matches single level, # matches all levels (must be last).',
            default: ['device/#'],
          },
        },
        required: ['broker'],
        ui_hints: {
          field_order: ['broker', 'port', 'username', 'password', 'client_id', 'tls', 'ca_cert', 'client_cert', 'client_key', 'subscribe_topics'],
          display_names: {
            broker: 'Broker Address',
            port: 'Port',
            username: 'Username',
            password: 'Password',
            tls: 'Use TLS',
            ca_cert: 'CA Certificate',
            client_cert: 'Client Certificate',
            client_key: 'Client Private Key',
            client_id: 'Client ID',
            subscribe_topics: 'Subscribe Topics',
          },
          placeholders: {
            ca_cert: '-----BEGIN CERTIFICATE-----\n...\n-----END CERTIFICATE-----',
            client_cert: '-----BEGIN CERTIFICATE-----\n...\n-----END CERTIFICATE-----',
            client_key: '-----BEGIN PRIVATE KEY-----\n...\n-----END PRIVATE KEY-----',
            client_id: 'Auto-generated if empty',
          },
          help_texts: {
            ca_cert: 'Required for self-signed or private CA certificates. Leave empty for public CAs.',
            client_cert: 'Required for mutual TLS (mTLS) authentication.',
            client_key: 'Required for mutual TLS (mTLS) authentication.',
            client_id: 'Unique identifier for this MQTT connection. Auto-generated if not specified.',
            subscribe_topics: 'System auto-subscribes: device/+/+/uplink, device/+/+/downlink. Add extra custom topics here.',
          },
          visibility_rules: [
            {
              field: 'tls',
              condition: 'equals',
              value: true,
              then_show: ['ca_cert', 'client_cert', 'client_key'],
            },
          ],
          // External brokers cannot detect device MQTT transport status
          // (NeoMind is not the broker owner). Show a notice so users
          // understand the 4-state model degrades to 3-state here.
          notice: {
            i18nKey: 'plugins:mqttExternalBrokerNotice',
          },
        },
      }
    case 'webhook':
      return {
        type: 'object',
        properties: {
          webhook_url: {
            type: 'string',
            description: 'Webhook URL for devices',
          },
        },
        required: [],
        ui_hints: {
          field_order: ['webhook_url'],
          display_names: {
            webhook_url: 'Webhook URL',
          },
        },
      }
    default:
      return {
        type: 'object',
        properties: {},
        required: [],
        ui_hints: {},
      }
  }
}

/**
 * Convert AdapterType from API to UnifiedPluginType
 */
function toUnifiedPluginType(type: AdapterType): UnifiedPluginType {
  const schema = getAdapterSchema(type.id)

  return {
    id: type.id,
    type: 'device_adapter',
    name: type.name,
    description: type.description,
    icon: type.icon, // Icon name string
    color: type.icon_bg,
    config_schema: schema,
    can_add_multiple: type.can_add_multiple,
    builtin: type.builtin,
  }
}

/** Token display with masked value and copy button */
function WebhookTokenDisplay({ token }: { token?: string }) {
  const { t } = useTranslation(['devices'])
  const { toast } = useToast()
  const [copied, setCopied] = useState(false)

  if (!token) {
    return (
      <div className="flex items-center gap-2 text-xs text-muted-foreground">
        <KeyRound className="h-3 w-3" />
        {t('devices:add.webhookTokenNone')}
      </div>
    )
  }

  return (
    <div className="flex items-center gap-2 min-w-0">
      <KeyRound className="h-3 w-3 text-muted-foreground shrink-0" />
      <code className="text-xs font-mono flex-1 min-w-0 truncate bg-muted px-1.5 py-0.5 rounded">
        {token.slice(0, 8)}{'•'.repeat(8)}{token.slice(-4)}
      </code>
      <Button
        variant="ghost"
        size="sm"
        className="h-6 w-6 p-0 shrink-0"
        onClick={async () => {
          await navigator.clipboard.writeText(token)
          setCopied(true)
          toast({ title: t('devices:add.webhookTokenCopied') })
          setTimeout(() => setCopied(false), 2000)
        }}
      >
        {copied ? <Check className="h-3 w-3" /> : <Copy className="h-3 w-3" />}
      </Button>
    </div>
  )
}

export function UnifiedDeviceConnectionsTab() {
  const { t } = useTranslation(['plugins', 'devices', 'common', 'settings'])
  const { handleError } = useErrorHandler()
  const { toast } = useToast()
  const serverUrl = useServerUrl()
  const [view, setView] = useState<View>('list')
  const [loading, setLoading] = useState(true)

  // Use hardcoded adapter types instead of fetching from API
  const adapterTypes = ADAPTER_TYPES

  // Data states
  const [mqttStatus, setMqttStatus] = useState<any>(null)
  const [externalBrokers, setExternalBrokers] = useState<any[]>([])
  const [devices, setDevices] = useState<any[]>([])
  const [webhookUrlCopied, setWebhookUrlCopied] = useState(false)

  // Config dialog state
  const [configDialogOpen, setConfigDialogOpen] = useState(false)
  const [editingInstance, setEditingInstance] = useState<PluginInstance | null>(null)
  const [testResults, setTestResults] = useState<Record<string, { success: boolean; message: string }>>({})
  const [testingId, setTestingId] = useState<string | null>(null)
  const [brokerConfigDialogOpen, setBrokerConfigDialogOpen] = useState(false)

  // Delete confirmation dialog state
  const [deleteDialogOpen, setDeleteDialogOpen] = useState(false)
  const [instanceToDelete, setInstanceToDelete] = useState<PluginInstance | null>(null)
  const [deleting, setDeleting] = useState(false)

  const [selectedType, setSelectedType] = useState<UnifiedPluginType | null>(null)

  // Load connection data on mount
  useEffect(() => {
    loadData()
  }, [])

  const loadData = async () => {
    setLoading(true)
    try {
      // Load connection data in parallel
      const [mqttResult, brokersResult, devicesResult] = await Promise.allSettled([
        api.getMqttStatus(),
        api.getBrokers(),
        api.getDevices(),
      ])

      if (mqttResult.status === 'fulfilled') {
        setMqttStatus(mqttResult.value.status)
      }

      if (brokersResult.status === 'fulfilled') {
        setExternalBrokers(brokersResult.value.brokers || [])
      }

      if (devicesResult.status === 'fulfilled') {
        setDevices(devicesResult.value.devices || [])
      }
    } catch (error) {
      handleError(error, { operation: 'Load device connections data', showToast: false })
      setMqttStatus(null)
      setExternalBrokers([])
      setDevices([])
    } finally {
      setLoading(false)
    }
  }

  const getDeviceCount = (type: string) => {
    if (type === 'mqtt') {
      // Count all MQTT devices (builtin + all external brokers)
      return devices.filter((d: any) =>
        !d.plugin_id || d.plugin_id === 'internal-mqtt' || d.plugin_id === 'builtin' ||
        externalBrokers.some((b: any) => b.id === d.plugin_id)
      ).length
    }
    if (type === 'webhook') {
      // Count webhook devices
      return devices.filter((d: any) => d.adapter_type === 'webhook').length
    }
    return 0
  }

  const getConnectionStatus = (type: string) => {
    if (type === 'mqtt') {
      // Connected if builtin OR any external broker is connected
      return (mqttStatus?.connected || false) || externalBrokers.some((b) => b.connected)
    }
    if (type === 'webhook') {
      // Always "available" - built-in endpoint
      return getDeviceCount(type) > 0
    }
    return false
  }

  const getWebhookUrl = () => {
    // Use canonical serverUrl (LAN IP in Tauri, window.location.origin in browser)
    // Don't use server_ip:port as it may not be accessible from external networks
    return `${serverUrl}/api/devices/{device_id}/webhook`
  }

  const copyWebhookUrl = async () => {
    const url = getWebhookUrl()
    await navigator.clipboard.writeText(url)
    setWebhookUrlCopied(true)
    toast({
      title: 'Webhook URL copied to clipboard',
      description: 'Replace {device_id} with your actual device ID',
    })
    setTimeout(() => setWebhookUrlCopied(false), 2000)
  }

  // Handle type selection
  const handleTypeSelect = (typeKey: string) => {
    const adapterType = adapterTypes.find(at => at.id === typeKey)
    if (adapterType) {
      setSelectedType(toUnifiedPluginType(adapterType))
      setView('detail')
    }
  }

  // Handle create
  const handleCreate = async (name: string, config: Record<string, unknown>) => {
    const type = selectedType!

    if (type.id === 'mqtt') {
      // Helper to get optional string value - only include if defined and non-empty
      const getOptionalString = (value: unknown): string | undefined => {
        if (value === undefined || value === null || value === '') return undefined
        return String(value)
      }

      const data: any = {
        name,
        broker: config.broker,
        port: config.port || 1883,
        username: getOptionalString(config.username),
        password: getOptionalString(config.password),
        tls: config.tls || false,
        ca_cert: getOptionalString(config.ca_cert),
        client_cert: getOptionalString(config.client_cert),
        client_key: getOptionalString(config.client_key),
        client_id: getOptionalString(config.client_id),
        enabled: true,
        subscribe_topics: config.subscribe_topics as string[] | undefined,
      }
      await api.createBroker(data)
    }

    return name
  }

  // Handle update
  const handleUpdate = async (id: string, config: Record<string, unknown>) => {
    const type = selectedType!

    if (type.id === 'mqtt') {
      const broker = externalBrokers.find((b) => b.id === id)
      if (!broker) throw new Error('Broker not found')

      // Helper to get optional string value - only include if defined and non-empty
      const getOptionalString = (value: unknown): string | undefined => {
        if (value === undefined || value === null || value === '') return undefined
        return String(value)
      }

      // Use existing broker name since it's not editable in the form
      const name = broker.name || (config.name as string)

      await api.updateBroker(id, {
        name,
        broker: config.broker as string,
        port: config.port as number,
        tls: config.tls as boolean,
        username: getOptionalString(config.username),
        password: getOptionalString(config.password),
        ca_cert: getOptionalString(config.ca_cert),
        client_cert: getOptionalString(config.client_cert),
        client_key: getOptionalString(config.client_key),
        client_id: getOptionalString(config.client_id),
        enabled: broker.enabled,
        subscribe_topics: config.subscribe_topics as string[] | undefined,
      })
    } else {
      throw new Error('Editing not supported for this adapter type')
    }
  }

  // Handle delete - show confirmation dialog
  const handleDelete = async (_id: string) => {
    const instance = editingInstance

    if (!instance) return

    // Set instance for deletion and show confirmation dialog
    setInstanceToDelete(instance)
    setDeleteDialogOpen(true)
  }

  // Confirm delete action
  const confirmDelete = async () => {
    if (!instanceToDelete) return

    setDeleting(true)
    try {
      if (instanceToDelete.plugin_type === 'mqtt') {
        await api.deleteBroker(instanceToDelete.id)
      } else {
        throw new Error('Cannot delete built-in adapter')
      }

      // Success
      toast({
        title: t('common:success', { defaultValue: 'Success' }),
        description: t('plugins:deleteSuccess', { defaultValue: 'Broker deleted successfully' }),
      })

      // Close dialog and refresh data
      setDeleteDialogOpen(false)
      setInstanceToDelete(null)
      await loadData()
    } catch (error) {
      handleError(error, { operation: 'Delete broker', showToast: true })
      showErrorToast(toast, error, t('common:failed', { defaultValue: 'Failed' }))
    } finally {
      setDeleting(false)
    }
  }

  // Handle test
  const handleTest = async (id: string) => {
    // Check by id prefix or against externalBrokers list — works from both dialog and card list
    const isExternalBroker = id !== 'builtin' && externalBrokers.some((b) => b.id === id)

    if (isExternalBroker || editingInstance?.plugin_type === 'mqtt') {
      const result = await api.testBroker(id)
      return {
        success: result.success,
        message: result.success
          ? 'Connection successful'
          : (result.message || 'Connection failed'),
      }
    }

    return {
      success: true,
      message: 'Test not supported for this adapter type',
    }
  }

  // Loading state
  if (loading) {
    return <LoadingState variant="page" text={t('common:loading')} />
  }

  // Empty state - when no adapter types are available
  if (adapterTypes.length === 0) {
    return (
      <EmptyState
        icon="device"
        title={t('plugins:noAdapters')}
        description={t('plugins:noAdaptersDesc')}
        action={{ label: t('common:retry'), onClick: loadData, icon: <Loader2 className="h-4 w-4" /> }}
      />
    )
  }

  // ========== LIST VIEW ==========
  if (view === 'list') {
    return (
      <>
        {/* Connection Type Cards Grid - Dynamically loaded */}
        <div className="grid gap-4 grid-cols-[repeat(auto-fill,minmax(260px,1fr))]">
          {adapterTypes.map((type) => {
            const isActive = getConnectionStatus(type.id)
            const deviceCount = getDeviceCount(type.id)
            const IconComponent = ICON_MAP[type.icon] || Server

            return (
              <Card
                key={type.id}
                className={cn(
                  "cursor-pointer transition-all duration-200 hover:shadow-md",
                  isActive && "border-success border-2"
                )}
                onClick={() => handleTypeSelect(type.id)}
              >
                <CardHeader className="pb-3">
                  <div className={cn("flex items-center justify-center w-12 h-12 rounded-lg", type.icon_bg)}>
                    <IconComponent className="h-6 w-6" />
                  </div>
                  <CardTitle className="text-base mt-3">{type.name}</CardTitle>
                  <CardDescription className="mt-1 text-xs line-clamp-2 min-h-[2.5em]">
                    {type.description}
                  </CardDescription>
                </CardHeader>
                <CardContent className="text-sm">
                  <div className="flex justify-between items-center">
                    <span className="text-muted-foreground">{t('plugins:llm.status')}:</span>
                    <span className={isActive ? "text-success dark:text-success font-medium" : "text-muted-foreground font-medium"}>
                      {isActive ? t('plugins:llm.running') : t('plugins:llm.notConfigured')}
                    </span>
                  </div>
                  <div className="flex justify-between items-center mt-2">
                    <span className="text-muted-foreground">{t('plugins:llm.devices')}:</span>
                    <span className="font-medium">{deviceCount}</span>
                  </div>
                </CardContent>
              </Card>
            )
          })}
        </div>
      </>
    )
  }

  // ========== DETAIL VIEW ==========
  if (view === 'detail' && selectedType) {
    const adapterType = adapterTypes.find(at => at.id === selectedType.id) || adapterTypes[0]

    // Special handling for Webhook - show usage info instead of instances
    if (selectedType.id === 'webhook') {
      const IconComponent = ICON_MAP[adapterType?.icon || 'Webhook'] || Webhook

      return (
        <>
          {/* Header with back button — sticky (see shared ListToolbar). */}
          <ListToolbar
            onBack={() => setView('list')}
            backLabel={t('plugins:llm.back')}
            icon={<IconComponent className="h-5 w-5 sm:h-6 sm:w-6" />}
            iconBg={adapterType?.icon_bg || ""}
            title={adapterType?.name || ""}
            description={adapterType?.description}
            responsiveIcon
          />

          {/* Webhook Info Card */}
          <Card className="mb-6">
            <CardHeader>
              <CardTitle>How Webhook Works</CardTitle>
              <CardDescription>
                Devices push data to your server via HTTP POST - no connection management needed
              </CardDescription>
            </CardHeader>
            <CardContent className="space-y-6">
              {/* Webhook URL */}
              <div className="space-y-2">
                <Label>Webhook URL</Label>
                <div className="flex gap-2 items-center">
                  <Input
                    readOnly
                    value={getWebhookUrl()}
                    className="font-mono text-xs sm:text-sm min-w-0"
                  />
                  <Button
                    size="sm"
                    variant="outline"
                    onClick={copyWebhookUrl}
                    className="shrink-0"
                  >
                    {webhookUrlCopied ? <Check className="h-4 w-4" /> : <Copy className="h-4 w-4" />}
                  </Button>
                </div>
                <p className="text-xs text-muted-foreground">
                  Replace <code className="bg-muted px-1 rounded">{'{device_id}'}</code> with your actual device ID
                </p>
              </div>

              {/* Request Format */}
              <div className="space-y-2">
                <Label>Request Format</Label>
                <div className="rounded-lg p-3 sm:p-4 -mx-1 sm:mx-0">
                  <pre className="text-xs sm:text-sm overflow-x-auto whitespace-pre">
{`POST ${getWebhookUrl()}

{
  "timestamp": 1234567890,
  "quality": 1.0,
  "data": {
    "temperature": 23.5,
    "humidity": 65
  }
}`}
                  </pre>
                </div>
              </div>

              {/* Response Format */}
              <div className="space-y-2">
                <Label>Response Format</Label>
                <div className="rounded-lg p-3 sm:p-4 -mx-1 sm:mx-0">
                  <pre className="text-xs sm:text-sm overflow-x-auto whitespace-pre">
{`{
  "success": true,
  "device_id": "sensor01",
  "metrics_received": 2,
  "timestamp": 1234567890
}`}
                  </pre>
                </div>
              </div>

              {/* Devices using webhook */}
              <div className="space-y-2">
                <Label>Devices Using Webhook ({getDeviceCount('webhook')})</Label>
                {getDeviceCount('webhook') > 0 ? (
                  <div className="space-y-2">
                    {devices.filter((d: any) => d.adapter_type === 'webhook').map((device: any) => {
                      const token = (device.config?.webhook_token || device.connection_config?.webhook_token) as string | undefined

                      return (
                        <div key={device.id} className="p-3 rounded border space-y-2">
                          <div className="flex items-center justify-between gap-2">
                            <div className="min-w-0 flex-1">
                              <div className="font-medium text-sm truncate">{device.name || device.id}</div>
                              <div className="text-xs text-muted-foreground font-mono truncate">
                                {getWebhookUrl().replace('{device_id}', device.id)}
                              </div>
                            </div>
                            <Badge variant={device.online ? 'default' : 'secondary'}>
                              {device.online ? 'Online' : 'Offline'}
                            </Badge>
                          </div>
                          <WebhookTokenDisplay token={token} />
                        </div>
                      )
                    })}
                  </div>
                ) : (
                  <p className="text-sm text-muted-foreground">No devices using webhook yet</p>
                )}
              </div>

              {/* Authentication guide */}
              <div className="space-y-2">
                <Label>{t('devices:add.webhookAuthHeader')}</Label>
                <p className="text-xs text-muted-foreground">{t('devices:add.webhookAuthDesc')}</p>
                <div className="rounded-lg p-3 sm:p-4 -mx-1 sm:mx-0 space-y-2">
                  <div>
                    <span className="text-xs font-medium">Authorization Header:</span>
                    <pre className="text-xs font-mono">Authorization: Bearer {'<token>'}</pre>
                  </div>
                  <div>
                    <span className="text-xs font-medium">Query Parameter:</span>
                    <pre className="text-xs font-mono">?token={'<token>'}</pre>
                  </div>
                </div>
              </div>
            </CardContent>
          </Card>
        </>
      )
    }

    // Get instances for this type
    let pluginInstances: PluginInstance[] = []

    if (selectedType.id === 'mqtt') {
      // Unified MQTT: builtin + external brokers
      pluginInstances = []

      // Add builtin broker
      if (mqttStatus?.connected || mqttStatus !== null) {
        pluginInstances.push({
          id: 'builtin',
          name: t('devices:mqtt.builtinMqtt.name'),
          plugin_type: 'mqtt',
          enabled: true,
          running: mqttStatus?.connected || false,
          isBuiltin: true,
          config: {
            listen_address: mqttStatus?.listen_address,
            listen_port: mqttStatus?.listen_port,
          } as Record<string, unknown>,
          status: {
            connected: mqttStatus?.connected || false,
          },
        })
      }

      // Add external brokers
      pluginInstances.push(...externalBrokers.map(brokerToInstance))
    }

    const IconComponent = ICON_MAP[adapterType?.icon || 'Server'] || Server

    return (
      <>
        {/* Header with back button — sticky (see shared ListToolbar). */}
        <ListToolbar
          onBack={() => setView('list')}
          backLabel={t('plugins:llm.back')}
          icon={<IconComponent className="h-6 w-6" />}
          iconBg={adapterType?.icon_bg || ""}
          title={adapterType?.name || ""}
          description={adapterType?.description}
        />

        {/* Instance Cards */}
        {pluginInstances.length === 0 ? (
          <Card className="border-dashed">
            <CardContent className="flex flex-col items-center justify-center py-12">
              <div className={cn("flex items-center justify-center w-16 h-16 rounded-lg mb-4", adapterType?.icon_bg)}>
                <IconComponent className="h-8 w-8" />
              </div>
              <h3 className="text-lg font-semibold mb-1">
                {t('plugins:llm.noInstanceYet', { name: adapterType?.name })}
              </h3>
              <p className="text-sm text-muted-foreground mb-4">
                {t('plugins:llm.configureToStart', { name: adapterType?.name })}
              </p>
              {selectedType.id === 'mqtt' && (
                <Button onClick={() => {
                  setEditingInstance(null)
                  setConfigDialogOpen(true)
                }}>
                  <Server className="mr-2 h-4 w-4" />
                  {t('devices:connections.addConnection')}
                </Button>
              )}
            </CardContent>
          </Card>
        ) : (
          <div className="grid gap-4 grid-cols-[minmax(0,1fr)] md:grid-cols-2">
            {pluginInstances.map((instance) => {
              const testResult = testResults[instance.id]
              const isMqtt = instance.plugin_type === 'mqtt'
              const isBuiltin = (instance as any).isBuiltin

              return (
                <Card
                  key={instance.id}
                  className={cn(
                    "transition-all duration-200 hover:shadow-md",
                    instance.running && "border-success"
                  )}
                >
                  <CardHeader className="pb-3">
                    <div className="flex items-start justify-between">
                      <div className="flex-1 min-w-0">
                        <div className="flex items-center gap-2 mb-1 min-w-0">
                          <CardTitle className="text-base truncate min-w-0">{instance.name}</CardTitle>
                          {isBuiltin && (
                            <Badge variant="outline" className="text-xs shrink-0">{t('devices:onboarding.builtin')}</Badge>
                          )}
                          {instance.running && (
                            <Badge variant="default" className="text-xs shrink-0">{t('plugins:llm.running')}</Badge>
                          )}
                          {instance.enabled && !instance.running && (
                            <Badge variant="outline" className="text-xs shrink-0">{t('plugins:enabled')}</Badge>
                          )}
                        </div>
                        <CardDescription className="font-mono text-xs truncate">
                          {isMqtt && isBuiltin
                            ? `${mqttStatus?.server_ip || 'localhost'}:${mqttStatus?.listen_port || 1883}`
                            : `${instance.config?.broker}:${instance.config?.port}`}
                        </CardDescription>
                      </div>
                      <div className="flex items-center gap-1 shrink-0 ml-2">
                        {isMqtt && isBuiltin && (
                          <Button
                            variant="ghost"
                            size="sm"
                            className="h-8 w-8 p-0"
                            onClick={(e) => {
                              e.stopPropagation()
                              setBrokerConfigDialogOpen(true)
                            }}
                            aria-label={t('settings:broker.settings')}
                          >
                            <Settings className="h-4 w-4" />
                          </Button>
                        )}
                        {isMqtt && !isBuiltin && (
                          <>
                            <Button
                              variant="ghost"
                              size="sm"
                              className="h-8 w-8 p-0"
                              disabled={testingId === instance.id}
                              onClick={async () => {
                                setTestingId(instance.id)
                                try {
                                  const result = await handleTest(instance.id)
                                  setTestResults((prev) => ({
                                    ...prev,
                                    [instance.id]: result,
                                  }))
                                } finally {
                                  setTestingId(null)
                                }
                              }}
                              title={t('plugins:testConnection')}
                            >
                              {testingId === instance.id ? (
                                <Loader2 className="h-4 w-4 animate-spin" />
                              ) : (
                                <TestTube className="h-4 w-4" />
                              )}
                            </Button>
                            <Button
                              variant="ghost"
                              size="sm"
                              className="h-8 w-8 p-0"
                              onClick={() => {
                                setEditingInstance(instance)
                                setConfigDialogOpen(true)
                              }}
                              title={t('plugins:edit')}
                            >
                              <Edit className="h-4 w-4" />
                            </Button>
                          </>
                        )}
                        {!isBuiltin && (
                          <Button
                            variant="ghost"
                            size="sm"
                            className="h-8 w-8 p-0 text-error hover:text-error"
                            onClick={() => {
                              setEditingInstance(instance)
                              handleDelete(instance.id)
                            }}
                            title={t('plugins:delete')}
                          >
                            <Trash2 className="h-4 w-4" />
                          </Button>
                        )}
                      </div>
                    </div>
                  </CardHeader>

                  <CardContent className="pb-3">
                    <div className="space-y-2 text-sm">
                      {testResult && (
                        <div className={cn(
                          "text-xs p-2 rounded",
                          testResult.success
                            ? "bg-success-light text-success dark:bg-success-light dark:text-success"
                            : "bg-error-light text-error"
                        )}>
                          {testResult.message}
                        </div>
                      )}
                      {instance.status?.error && (
                        <div className="text-xs text-error">
                          {instance.status.error}
                        </div>
                      )}
                    </div>
                  </CardContent>
                </Card>
              )
            })}
          </div>
        )}

        {/* Add Instance Button */}
        {selectedType.can_add_multiple && (
          <div className="mt-4">
            <Button onClick={() => {
              setEditingInstance(null)
              setConfigDialogOpen(true)
            }}>
              <Server className="mr-2 h-4 w-4" />
              {t('devices:connections.addConnection')}
            </Button>
          </div>
        )}

        {/* Unified Config Dialog */}
        <UniversalPluginConfigDialog
          open={configDialogOpen}
          onOpenChange={(open) => {
            setConfigDialogOpen(open)
            if (!open) {
              setEditingInstance(null)
              setTestResults({})
            }
          }}
          pluginType={selectedType}
          instances={pluginInstances}
          editingInstance={editingInstance}
          onCreate={handleCreate}
          onUpdate={handleUpdate}
          onDelete={handleDelete}
          onTest={handleTest}
          onRefresh={loadData}
          testResults={testResults}
          setTestResults={setTestResults}
        />

        {/* Embedded Broker Config Dialog */}
        <EmbeddedBrokerConfigDialog
          open={brokerConfigDialogOpen}
          onOpenChange={setBrokerConfigDialogOpen}
          onConfigSaved={loadData}
        />

        {/* Delete Confirmation Dialog */}
        <Dialog open={deleteDialogOpen} onOpenChange={setDeleteDialogOpen}>
          <DialogContent className="sm:max-w-md z-[200]">
            <DialogHeader>
              <DialogTitle className="flex items-center gap-2">
                <AlertTriangle className="h-5 w-5 text-error" />
                {t('plugins:deleteConfirmTitle', { defaultValue: 'Delete Broker?' })}
              </DialogTitle>
              <DialogDescription>
                {t('plugins:deleteConfirmDesc', {
                  defaultValue: 'Are you sure you want to delete "{{name}}"? This action cannot be undone.',
                  name: instanceToDelete?.name || ''
                })}
              </DialogDescription>
            </DialogHeader>

            <DialogContentBody className="px-4 pt-6 pb-4 sm:px-6">
              <p className="text-sm text-muted-foreground">
                {t('plugins:deleteWarning', { defaultValue: 'This action cannot be undone.' })}
              </p>
            </DialogContentBody>
            <DialogFooter>
              <Button
                variant="outline"
                onClick={() => setDeleteDialogOpen(false)}
                disabled={deleting}
              >
                {t('common:cancel', { defaultValue: 'Cancel' })}
              </Button>
              <Button
                variant="destructive"
                onClick={confirmDelete}
                disabled={deleting}
              >
                {deleting ? (
                  <>
                    <Loader2 className="h-4 w-4 mr-2 animate-spin" />
                    {t('common:deleting', { defaultValue: 'Deleting...' })}
                  </>
                ) : (
                  t('plugins:delete', { defaultValue: 'Delete' })
                )}
              </Button>
            </DialogFooter>
          </DialogContent>
        </Dialog>
      </>
    )
  }

  return null
}

/**
 * Convert external broker to PluginInstance
 */
function brokerToInstance(broker: any): PluginInstance {
  return {
    id: broker.id,
    name: broker.name,
    plugin_type: 'mqtt',
    enabled: broker.enabled ?? true,
    running: broker.connected ?? false,
    config: {
      broker: String(broker.host || broker.broker || ''),
      port: Number(broker.port || 1883),
      username: broker.username || '',
      password: broker.password || '',
      tls: broker.tls || false,
      ca_cert: broker.ca_cert || '',
      client_cert: broker.client_cert || '',
      client_key: broker.client_key || '',
      client_id: broker.client_id || '',
      subscribe_topics: broker.subscribe_topics || ['#'],
    } as Record<string, unknown>,
    status: {
      connected: broker.connected ?? false,
      error: broker.last_error,
    },
  }
}
