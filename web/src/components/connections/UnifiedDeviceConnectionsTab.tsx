import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { useErrorHandler } from '@/hooks/useErrorHandler'
import {
  ArrowLeft,
  Server,
  Edit,
  Trash2,
  TestTube,
  MoreVertical,
  Webhook,
  Radio,
  Copy,
  Check,
  LucideIcon,
  Loader2,
  AlertTriangle,
} from 'lucide-react'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import { EmptyState, LoadingState } from '@/components/shared'
import { cn } from '@/lib/utils'
import { api } from '@/lib/api'
import { UniversalPluginConfigDialog, type PluginInstance, type UnifiedPluginType } from '@/components/plugins/UniversalPluginConfigDialog'
import type { PluginConfigSchema, AdapterType } from '@/types'
import { useToast } from '@/hooks/use-toast'
import { ADAPTER_TYPES } from '@/constants/deviceAdapters'

// Icon mapping for adapter types
const ICON_MAP: Record<string, LucideIcon> = {
  Server,
  Webhook,
  Radio,
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
          subscribe_topics: {
            type: 'array',
            description: 'Topics to subscribe (one per line). Wildcards: + matches single level, # matches all levels (must be last). Examples: ne301/+, sensor/+/data, device/#',
            default: ['ne301/#'],
          },
        },
        required: ['broker'],
        ui_hints: {
          field_order: ['broker', 'port', 'username', 'password', 'tls', 'subscribe_topics'],
          display_names: {
            broker: 'Broker Address',
            port: 'Port',
            username: 'Username',
            password: 'Password',
            tls: 'Use TLS',
            subscribe_topics: 'Subscribe Topics',
          },
        },
      }
    case 'http':
      return {
        type: 'object',
        properties: {
          name: {
            type: 'string',
            description: 'Adapter name',
          },
          poll_interval: {
            type: 'number',
            description: 'Default poll interval (seconds)',
            default: 30,
            minimum: 1,
          },
          timeout: {
            type: 'number',
            description: 'Request timeout (seconds)',
            default: 10,
            minimum: 1,
          },
        },
        required: ['name'],
        ui_hints: {
          field_order: ['name', 'poll_interval', 'timeout'],
          display_names: {
            name: 'Adapter Name',
            poll_interval: 'Poll Interval (sec)',
            timeout: 'Timeout (sec)',
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

export function UnifiedDeviceConnectionsTab() {
  const { t } = useTranslation(['plugins', 'devices', 'common'])
  const { handleError } = useErrorHandler()
  const { toast } = useToast()
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
    if (type === 'http') {
      // Count HTTP polling devices
      return devices.filter((d: any) => d.adapter_type === 'http').length
    }
    return 0
  }

  const getConnectionStatus = (type: string) => {
    if (type === 'mqtt') {
      // Connected if builtin OR any external broker is connected
      return (mqttStatus?.connected || false) || externalBrokers.some((b) => b.connected)
    }
    if (type === 'webhook' || type === 'http') {
      // Always "available" - these are built-in endpoints
      return getDeviceCount(type) > 0
    }
    return false
  }

  const getWebhookUrl = () => {
    return `${window.location.origin}/api/devices/webhook/{device_id}`
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
      toast({
        title: t('common:failed', { defaultValue: 'Failed' }),
        description: (error as Error).message || t('plugins:deleteFailed', { defaultValue: 'Failed to delete broker' }),
        variant: 'destructive',
      })
    } finally {
      setDeleting(false)
    }
  }

  // Handle test
  const handleTest = async (id: string) => {
    const instance = editingInstance

    if (instance?.plugin_type === 'mqtt') {
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
        <div className="grid gap-4 grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 xl:grid-cols-4">
          {adapterTypes.map((type) => {
            const isActive = getConnectionStatus(type.id)
            const deviceCount = getDeviceCount(type.id)
            const IconComponent = ICON_MAP[type.icon] || Server

            return (
              <Card
                key={type.id}
                className={cn(
                  "cursor-pointer transition-all duration-200 hover:shadow-md",
                  isActive && "border-green-500 border-2"
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
                    <span className={isActive ? "text-green-600 dark:text-green-400 font-medium" : "text-muted-foreground font-medium"}>
                      {isActive ? t('plugins:llm.running') : t('plugins:llm.notConfigured')}
                    </span>
                  </div>
                  <div className="flex justify-between items-center mt-2">
                    <span className="text-muted-foreground">{t('plugins:llm.instances')}:</span>
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
          {/* Header with back button */}
          <div className="flex items-center gap-4 mb-6">
            <Button variant="ghost" size="sm" onClick={() => setView('list')} className="gap-1">
              <ArrowLeft className="h-4 w-4" />
              {t('plugins:llm.back')}
            </Button>
            <div className="flex items-center gap-3">
              <div className={cn("flex items-center justify-center w-12 h-12 rounded-lg", adapterType?.icon_bg)}>
                <IconComponent className="h-6 w-6" />
              </div>
              <div>
                <h2 className="text-2xl font-bold">{adapterType?.name}</h2>
                <p className="text-sm text-muted-foreground">{adapterType?.description}</p>
              </div>
            </div>
          </div>

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
                <div className="flex gap-2">
                  <Input
                    readOnly
                    value={getWebhookUrl()}
                    className="font-mono text-sm"
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
                <div className="rounded-lg bg-muted p-4">
                  <pre className="text-sm overflow-x-auto">
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
                <div className="rounded-lg bg-muted p-4">
                  <pre className="text-sm">
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
                    {devices.filter((d: any) => d.adapter_type === 'webhook').map((device: any) => (
                      <div key={device.id} className="flex items-center justify-between p-2 rounded border">
                        <div>
                          <div className="font-medium">{device.name || device.id}</div>
                          <div className="text-xs text-muted-foreground font-mono">
                            {getWebhookUrl().replace('{device_id}', device.id)}
                          </div>
                        </div>
                        <Badge variant={device.online ? 'default' : 'secondary'}>
                          {device.online ? 'Online' : 'Offline'}
                        </Badge>
                      </div>
                    ))}
                  </div>
                ) : (
                  <p className="text-sm text-muted-foreground">No devices using webhook yet</p>
                )}
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
          name: 'Internal Broker',
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
    } else if (selectedType.id === 'http') {
      // HTTP polling devices - show as "instances" since they are configured per device
      const httpDevices = devices.filter((d: any) => d.adapter_type === 'http')
      pluginInstances = httpDevices.map((device: any) => ({
        id: device.id,
        name: device.name || device.id,
        plugin_type: 'http',
        enabled: true,
        running: device.online || false,
        isBuiltin: false,
        config: {
          ...(device.connection_config || {}),
          _url: device.connection_config?.url,
          _poll_interval: device.connection_config?.poll_interval,
        },
        status: {
          connected: device.online || false,
        },
      }))
    }

    const IconComponent = ICON_MAP[adapterType?.icon || 'Server'] || Server

    return (
      <>
        {/* Header with back button */}
        <div className="flex items-center gap-4 mb-4">
          <Button variant="ghost" size="sm" onClick={() => setView('list')} className="gap-1">
            <ArrowLeft className="h-4 w-4" />
            {t('plugins:llm.back')}
          </Button>
          <div className="flex items-center gap-3">
            <div className={cn("flex items-center justify-center w-10 h-10 rounded-lg", adapterType?.icon_bg)}>
              <IconComponent className="h-6 w-6" />
            </div>
            <div>
              <h2 className="text-2xl font-bold">{adapterType?.name}</h2>
              <p className="text-sm text-muted-foreground">{adapterType?.description}</p>
            </div>
          </div>
        </div>

        {/* Instance Cards */}
        {pluginInstances.length === 0 ? (
          <Card className="border-dashed">
            <CardContent className="flex flex-col items-center justify-center py-12">
              <div className={cn("flex items-center justify-center w-16 h-16 rounded-lg mb-4", adapterType?.icon_bg)}>
                <IconComponent className="h-8 w-8" />
              </div>
              <h3 className="text-lg font-semibold mb-1">
                {selectedType.id === 'http' ? 'No HTTP polling devices configured' : t('plugins:llm.noInstanceYet', { name: adapterType?.name })}
              </h3>
              <p className="text-sm text-muted-foreground mb-4">
                {selectedType.id === 'http'
                  ? 'Add devices with HTTP adapter type to enable polling'
                  : t('plugins:llm.configureToStart', { name: adapterType?.name })
                }
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
          <div className="grid gap-4 md:grid-cols-2">
            {pluginInstances.map((instance) => {
              const testResult = testResults[instance.id]
              const isMqtt = instance.plugin_type === 'mqtt'
              const isHttp = instance.plugin_type === 'http'

              return (
                <Card
                  key={instance.id}
                  className={cn(
                    "transition-all duration-200 hover:shadow-md",
                    instance.running && "border-green-500"
                  )}
                >
                  <CardHeader className="pb-3">
                    <div className="flex items-start justify-between">
                      <div className="flex-1">
                        <div className="flex items-center gap-2 mb-1">
                          <CardTitle className="text-base">{instance.name}</CardTitle>
                          {(instance as any).isBuiltin && (
                            <Badge variant="outline" className="text-xs">内置</Badge>
                          )}
                          {instance.running && (
                            <Badge variant="default" className="text-xs">{t('plugins:llm.running')}</Badge>
                          )}
                          {instance.enabled && !instance.running && (
                            <Badge variant="outline" className="text-xs">{t('plugins:enabled')}</Badge>
                          )}
                        </div>
                        <CardDescription className="font-mono text-xs">
                          {isMqtt && (instance as any).isBuiltin
                            ? `${mqttStatus?.server_ip || 'localhost'}:${mqttStatus?.listen_port || 1883}`
                            : isHttp
                            ? `${(instance.config as any)?._url || 'N/A'} (${(instance.config as any)?._poll_interval || 30}s)`
                            : `${instance.config?.broker}:${instance.config?.port}`}
                        </CardDescription>
                      </div>
                      <DropdownMenu>
                        <DropdownMenuTrigger asChild>
                          <Button variant="ghost" size="sm" className="h-8 w-8 p-0">
                            <MoreVertical className="h-4 w-4" />
                          </Button>
                        </DropdownMenuTrigger>
                        <DropdownMenuContent align="end">
                          {isMqtt && !(instance as any).isBuiltin && (
                            <>
                              <DropdownMenuItem onClick={() => {
                                setEditingInstance(instance)
                                setConfigDialogOpen(true)
                              }}>
                                <Edit className="mr-2 h-4 w-4" />
                                {t('plugins:edit')}
                              </DropdownMenuItem>
                              <DropdownMenuItem onClick={async () => {
                                const result = await handleTest(instance.id)
                                setTestResults((prev) => ({
                                  ...prev,
                                  [instance.id]: result,
                                }))
                              }}>
                                <TestTube className="mr-2 h-4 w-4" />
                                {t('plugins:testConnection')}
                              </DropdownMenuItem>
                            </>
                          )}
                          {/* Hide delete for builtin MQTT instance */}
                          {!(isMqtt && (instance as any).isBuiltin) && (
                            <DropdownMenuItem
                              className="text-destructive focus:text-destructive"
                              onClick={() => {
                                setEditingInstance(instance)
                                handleDelete(instance.id)
                              }}
                            >
                              <Trash2 className="mr-2 h-4 w-4" />
                              {t('plugins:delete')}
                            </DropdownMenuItem>
                          )}
                        </DropdownMenuContent>
                      </DropdownMenu>
                    </div>
                  </CardHeader>

                  <CardContent className="pb-3">
                    <div className="space-y-2 text-sm">
                      {testResult && (
                        <div className={cn(
                          "text-xs p-2 rounded",
                          testResult.success
                            ? "bg-green-50 text-green-700 dark:bg-green-900 dark:text-green-300"
                            : "bg-red-50 text-red-700 dark:bg-red-900 dark:text-red-300"
                        )}>
                          {testResult.message}
                        </div>
                      )}
                      {instance.status?.error && (
                        <div className="text-xs text-destructive">
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

        {/* Delete Confirmation Dialog */}
        <Dialog open={deleteDialogOpen} onOpenChange={setDeleteDialogOpen}>
          <DialogContent className="max-w-md">
            <DialogHeader>
              <DialogTitle className="flex items-center gap-2">
                <AlertTriangle className="h-5 w-5 text-destructive" />
                {t('plugins:deleteConfirmTitle', { defaultValue: 'Delete Broker?' })}
              </DialogTitle>
              <DialogDescription>
                {t('plugins:deleteConfirmDesc', {
                  defaultValue: 'Are you sure you want to delete "{{name}}"? This action cannot be undone.',
                  name: instanceToDelete?.name || ''
                })}
              </DialogDescription>
            </DialogHeader>
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
      broker: String(broker.broker || ''),
      port: Number(broker.port || 1883),
      username: broker.username || '',
      tls: broker.tls || false,
      subscribe_topics: broker.subscribe_topics || ['#'],
    } as Record<string, unknown>,
    status: {
      connected: broker.connected ?? false,
      error: broker.last_error,
    },
  }
}
