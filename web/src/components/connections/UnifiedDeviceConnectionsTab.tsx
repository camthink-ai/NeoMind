import { useEffect, useState, useRef } from 'react'
import { useTranslation } from 'react-i18next'
import {
  Loader2,
  ArrowLeft,
  Server,
  Network,
  Home,
  Wifi,
  Edit,
  Trash2,
  TestTube,
  MoreVertical,
} from 'lucide-react'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu'
import { cn } from '@/lib/utils'
import { api } from '@/lib/api'
import { UniversalPluginConfigDialog, type PluginInstance, type UnifiedPluginType } from '@/components/plugins/UniversalPluginConfigDialog'
import type { PluginConfigSchema } from '@/types'

type View = 'list' | 'detail'

interface UnifiedDeviceConnectionsTabProps {
  onRegisterAdapter?: (adapter: any) => Promise<void>
  onDeleteAdapter?: (id: string) => Promise<void>
}

// Device Adapter type info
const ADAPTER_TYPE_INFO: Record<string, {
  id: string
  name: string
  icon: React.ReactNode
  iconBg: string
  canAddMultiple: boolean
  builtin: boolean
  description: string
}> = {
  builtinMqtt: {
    id: 'builtin-mqtt',
    name: 'Built-in MQTT',
    icon: <Server className="h-6 w-6" />,
    iconBg: 'bg-green-100 text-green-700 dark:bg-green-900/20 dark:text-green-400',
    canAddMultiple: false,
    builtin: true,
    description: 'Internal embedded MQTT broker for device communication',
  },
  externalMqtt: {
    id: 'external-mqtt',
    name: 'External MQTT',
    icon: <Network className="h-6 w-6" />,
    iconBg: 'bg-blue-100 text-blue-700 dark:bg-blue-900/20 dark:text-blue-400',
    canAddMultiple: true,
    builtin: false,
    description: 'Connect to external MQTT brokers',
  },
  modbus: {
    id: 'modbus',
    name: 'Modbus TCP',
    icon: <Wifi className="h-6 w-6" />,
    iconBg: 'bg-orange-100 text-orange-700 dark:bg-orange-900/20 dark:text-orange-400',
    canAddMultiple: true,
    builtin: false,
    description: 'Connect to Modbus TCP devices',
  },
  hass: {
    id: 'hass',
    name: 'Home Assistant',
    icon: <Home className="h-6 w-6" />,
    iconBg: 'bg-purple-100 text-purple-700 dark:bg-purple-900/20 dark:text-purple-400',
    canAddMultiple: false,
    builtin: false,
    description: 'Home Assistant discovery integration',
  },
}

// Config schemas for each adapter type
const getAdapterSchema = (adapterType: string): PluginConfigSchema => {
  switch (adapterType) {
    case 'external-mqtt':
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
        },
        required: ['broker'],
        ui_hints: {
          field_order: ['broker', 'port', 'username', 'password', 'tls'],
          display_names: {
            broker: 'Broker Address',
            port: 'Port',
            username: 'Username',
            password: 'Password',
            tls: 'Use TLS',
          },
        },
      }
    case 'modbus':
      return {
        type: 'object',
        properties: {
          host: {
            type: 'string',
            description: 'Modbus device IP address',
            format: 'hostname',
            default: '192.168.1.100',
          },
          port: {
            type: 'number',
            description: 'Modbus port',
            default: 502,
            minimum: 1,
            maximum: 65535,
          },
          slave_id: {
            type: 'number',
            description: 'Modbus slave ID',
            default: 1,
            minimum: 1,
            maximum: 247,
          },
        },
        required: ['host'],
        ui_hints: {
          field_order: ['host', 'port', 'slave_id'],
          display_names: {
            host: 'Host Address',
            port: 'Port',
            slave_id: 'Slave ID',
          },
        },
      }
    case 'hass':
      return {
        type: 'object',
        properties: {
          url: {
            type: 'string',
            description: 'Home Assistant URL',
            format: 'uri',
            default: 'http://homeassistant.local:8123',
          },
          token: {
            type: 'string',
            description: 'Long-lived access token',
            secret: true,
          },
          verify_ssl: {
            type: 'boolean',
            description: 'Verify SSL certificate',
            default: true,
          },
          auto_import: {
            type: 'boolean',
            description: 'Automatically import discovered devices',
            default: false,
          },
        },
        required: ['url', 'token'],
        ui_hints: {
          field_order: ['url', 'token', 'verify_ssl', 'auto_import'],
          display_names: {
            url: 'Home Assistant URL',
            token: 'Access Token',
            verify_ssl: 'Verify SSL',
            auto_import: 'Auto Import Devices',
          },
          help_texts: {
            url: 'URL of your Home Assistant server',
            token: 'Generate in Home Assistant: Profile > Long-Lived Access Tokens',
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
 * Convert adapter type to UnifiedPluginType
 */
function toUnifiedPluginType(typeKey: string): UnifiedPluginType {
  const info = ADAPTER_TYPE_INFO[typeKey]
  const schema = getAdapterSchema(typeKey)

  return {
    id: info.id,
    type: 'device_adapter',
    name: info.name,
    description: info.description,
    icon: info.icon,
    color: info.iconBg,
    config_schema: schema,
    can_add_multiple: info.canAddMultiple,
    builtin: info.builtin,
  }
}

/**
 * Convert external broker to PluginInstance
 */
function brokerToInstance(broker: any): PluginInstance {
  return {
    id: broker.id,
    name: broker.name,
    plugin_type: 'external-mqtt',
    enabled: broker.enabled ?? true,
    running: broker.connected ?? false,
    config: {
      broker: String(broker.broker || ''),
      port: Number(broker.port || 1883),
      username: broker.username || '',
      tls: broker.tls || false,
    } as Record<string, unknown>,
    status: {
      connected: broker.connected ?? false,
      error: broker.last_error,
    },
  }
}

/**
 * Convert Modbus adapter to PluginInstance
 */
function modbusToInstance(adapter: any): PluginInstance {
  return {
    id: adapter.id,
    name: adapter.name,
    plugin_type: 'modbus',
    enabled: adapter.enabled,
    running: adapter.running,
    config: (adapter.config || {}) as Record<string, unknown>,
    status: {
      connected: adapter.running,
    },
  }
}

export function UnifiedDeviceConnectionsTab({
  onRegisterAdapter,
  onDeleteAdapter,
}: UnifiedDeviceConnectionsTabProps) {
  const { t } = useTranslation(['plugins', 'devices', 'common'])
  const [view, setView] = useState<View>('list')
  const [loading, setLoading] = useState(true)
  const [schemaLoading, setSchemaLoading] = useState(false)

  // Data states
  const [mqttStatus, setMqttStatus] = useState<any>(null)
  const [externalBrokers, setExternalBrokers] = useState<any[]>([])
  const [modbusAdapters, setModbusAdapters] = useState<any[]>([])
  const [hassStatus, setHassStatus] = useState<any>(null)
  const [devices, setDevices] = useState<any[]>([])

  // Config dialog state
  const [configDialogOpen, setConfigDialogOpen] = useState(false)
  const [editingInstance, setEditingInstance] = useState<PluginInstance | null>(null)
  const [testResults, setTestResults] = useState<Record<string, { success: boolean; message: string }>>({})

  const hasFetched = useRef(false)
  useEffect(() => {
    if (!hasFetched.current) {
      hasFetched.current = true
      loadData()
    }
  }, [])

  const loadData = async () => {
    setLoading(true)
    try {
      const [mqttResult, brokersResult, hassResult, devicesResult, adaptersResult] = await Promise.allSettled([
        api.getMqttStatus(),
        api.getBrokers(),
        api.getHassDiscoveryStatus(),
        api.getDevices(),
        api.listDeviceAdapters(),
      ])

      if (mqttResult.status === 'fulfilled') {
        setMqttStatus(mqttResult.value.status)
      }

      if (brokersResult.status === 'fulfilled') {
        setExternalBrokers(brokersResult.value.brokers || [])
      }

      if (hassResult.status === 'fulfilled') {
        setHassStatus(hassResult.value)
      }

      if (devicesResult.status === 'fulfilled') {
        setDevices(devicesResult.value.devices || [])
      }

      if (adaptersResult.status === 'fulfilled') {
        const modbus = adaptersResult.value.adapters?.filter((a: any) => a.adapter_type === 'modbus') || []
        setModbusAdapters(modbus)
      }
    } catch (error) {
      console.error('Failed to load device connections data:', error)
    } finally {
      setLoading(false)
    }
  }

  const getDeviceCount = (type: string, id?: string) => {
    if (type === 'builtin-mqtt') {
      return devices.filter((d: any) =>
        !d.plugin_id || d.plugin_id === 'internal-mqtt' || d.plugin_id === 'builtin'
      ).length
    } else if (type === 'external-mqtt' && id) {
      return devices.filter((d: any) => d.plugin_id === id).length
    } else if (type === 'hass') {
      return devices.filter((d: any) => d.plugin_id === 'hass-discovery').length
    } else if (type === 'modbus' && id) {
      return devices.filter((d: any) => d.plugin_id === id).length
    }
    return 0
  }

  const getConnectionStatus = (type: string) => {
    if (type === 'builtin-mqtt') {
      return mqttStatus?.connected || false
    } else if (type === 'external-mqtt') {
      return externalBrokers.some((b) => b.connected)
    } else if (type === 'hass') {
      return hassStatus?.hass_integration?.connected || false
    } else if (type === 'modbus') {
      return modbusAdapters.some((a) => a.running)
    }
    return false
  }

  // Handle type selection
  const handleTypeSelect = (typeKey: string) => {
    setSchemaLoading(true)
    setSelectedType(toUnifiedPluginType(typeKey))
    setSchemaLoading(false)
    setView('detail')
  }

  const [selectedType, setSelectedType] = useState<UnifiedPluginType | null>(null)

  // Handle create
  const handleCreate = async (name: string, config: Record<string, unknown>) => {
    const type = selectedType!

    if (type.id === 'external-mqtt') {
      const data: any = {
        name,
        broker: config.broker,
        port: config.port || 1883,
        username: config.username,
        password: config.password,
        tls: config.tls || false,
        enabled: true,
      }
      await api.createBroker(data)
    } else if (type.id === 'modbus') {
      const data = {
        id: `modbus-${Date.now()}`,
        name,
        adapter_type: 'modbus',
        config: {
          host: config.host,
          port: config.port || 502,
          slave_id: config.slave_id || 1,
        },
        auto_start: true,
        enabled: true,
      }
      if (onRegisterAdapter) {
        await onRegisterAdapter(data)
      } else {
        await api.registerDeviceAdapter(data)
      }
    } else if (type.id === 'hass') {
      await api.connectHass({
        url: String(config.url),
        token: String(config.token),
        verify_ssl: Boolean(config.verify_ssl),
        auto_import: Boolean(config.auto_import),
      })
    }

    return name
  }

  // Handle update
  const handleUpdate = async (id: string, config: Record<string, unknown>) => {
    const type = selectedType!

    if (type.id === 'external-mqtt') {
      const broker = externalBrokers.find((b) => b.id === id)
      if (!broker) throw new Error('Broker not found')

      await api.updateBroker(id, {
        name: config.name as string,
        broker: config.broker as string,
        port: config.port as number,
        tls: config.tls as boolean,
        username: config.username as string,
        password: config.password as string,
        enabled: broker.enabled,
      })
    } else if (type.id === 'modbus') {
      await api.updatePluginConfig(id, {
        host: config.host,
        port: config.port,
        slave_id: config.slave_id,
      })
    } else {
      throw new Error('Editing not supported for this adapter type')
    }
  }

  // Handle delete
  const handleDelete = async (id: string) => {
    const instance = editingInstance

    if (instance?.plugin_type === 'external-mqtt') {
      await api.deleteBroker(id)
    } else if (instance?.plugin_type === 'modbus') {
      if (onDeleteAdapter) {
        await onDeleteAdapter(id)
      } else {
        await api.unregisterPlugin(id)
      }
    } else {
      throw new Error('Cannot delete built-in adapter')
    }
  }

  // Handle test
  const handleTest = async (id: string) => {
    const instance = editingInstance

    if (instance?.plugin_type === 'external-mqtt') {
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

  if (loading) {
    return (
      <div className="flex items-center justify-center py-12">
        <Loader2 className="h-8 w-8 animate-spin text-muted-foreground" />
      </div>
    )
  }

  if (schemaLoading) {
    return (
      <div className="flex items-center justify-center py-12">
        <Loader2 className="h-8 w-8 animate-spin text-muted-foreground" />
      </div>
    )
  }

  // ========== LIST VIEW ==========
  if (view === 'list') {
    return (
      <>
        {/* Header */}
        <div className="flex items-center justify-between mb-4">
          <div>
            <h2 className="text-2xl font-bold tracking-tight">{t('plugins:deviceConnections')}</h2>
            <p className="text-muted-foreground text-sm">
              {t('devices:connections.description')}
            </p>
          </div>
        </div>

        {/* Connection Type Cards Grid */}
        <div className="grid gap-4 grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 xl:grid-cols-4">
          {(Object.keys(ADAPTER_TYPE_INFO) as Array<keyof typeof ADAPTER_TYPE_INFO>).map((typeKey) => {
            const info = ADAPTER_TYPE_INFO[typeKey]
            const isActive = getConnectionStatus(info.id)
            const deviceCount = getDeviceCount(info.id)

            return (
              <Card
                key={typeKey}
                className={cn(
                  "cursor-pointer transition-all duration-200 hover:shadow-md",
                  isActive && "border-green-500 border-2"
                )}
                onClick={() => handleTypeSelect(typeKey)}
              >
                <CardHeader className="pb-3">
                  <div className={cn("flex items-center justify-center w-12 h-12 rounded-lg", info.iconBg)}>
                    {info.icon}
                  </div>
                  <CardTitle className="text-base mt-3">{info.name}</CardTitle>
                  <CardDescription className="mt-1 text-xs line-clamp-2 min-h-[2.5em]">
                    {info.description}
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
    const info = ADAPTER_TYPE_INFO[selectedType.id as keyof typeof ADAPTER_TYPE_INFO] || ADAPTER_TYPE_INFO.builtinMqtt

    // Get instances for this type
    let pluginInstances: PluginInstance[] = []

    if (selectedType.id === 'builtin-mqtt') {
      // Single instance for built-in MQTT
      if (mqttStatus?.connected || mqttStatus !== null) {
        pluginInstances = [{
          id: 'internal-mqtt',
          name: 'Internal MQTT Broker',
          plugin_type: 'builtin-mqtt',
          enabled: true,
          running: mqttStatus?.connected || false,
          config: {
            listen_address: mqttStatus?.listen_address,
            listen_port: mqttStatus?.listen_port,
          },
          status: {
            connected: mqttStatus?.connected || false,
          },
        }]
      }
    } else if (selectedType.id === 'external-mqtt') {
      pluginInstances = externalBrokers.map(brokerToInstance)
    } else if (selectedType.id === 'modbus') {
      pluginInstances = modbusAdapters.map(modbusToInstance)
    } else if (selectedType.id === 'hass') {
      if (hassStatus?.hass_integration?.url || hassStatus?.hass_integration?.connected) {
        pluginInstances = [{
          id: 'hass-discovery',
          name: 'Home Assistant',
          plugin_type: 'hass',
          enabled: hassStatus?.hass_integration?.enabled || false,
          running: hassStatus?.hass_integration?.connected || false,
          config: {
            url: hassStatus?.hass_integration?.url,
          },
          status: {
            connected: hassStatus?.hass_integration?.connected || false,
          },
        }]
      }
    }

    return (
      <>
        {/* Header with back button */}
        <div className="flex items-center gap-4 mb-4">
          <Button variant="ghost" size="sm" onClick={() => setView('list')} className="gap-1">
            <ArrowLeft className="h-4 w-4" />
            {t('plugins:llm.back')}
          </Button>
          <div className="flex items-center gap-3">
            <div className={cn("flex items-center justify-center w-10 h-10 rounded-lg", info.iconBg)}>
              {info.icon}
            </div>
            <div>
              <h2 className="text-2xl font-bold">{info.name}</h2>
              <p className="text-sm text-muted-foreground">{info.description}</p>
            </div>
          </div>
        </div>

        {/* Builtin MQTT Status Card (special case) */}
        {selectedType.id === 'builtin-mqtt' && mqttStatus && (
          <Card className={cn("transition-all duration-200", mqttStatus.connected && "border-green-500")}>
            <CardHeader className="pb-3">
              <div className="flex items-start justify-between">
                <div className="flex items-center gap-3">
                  <div className="p-2 rounded-lg bg-green-100 text-green-700 dark:bg-green-900/20 dark:text-green-400">
                    <Server className="h-5 w-5" />
                  </div>
                  <div>
                    <CardTitle className="text-base">{t('devices:connections.builtinMqtt.title')}</CardTitle>
                    <CardDescription className="text-xs">
                      {mqttStatus.server_ip || 'localhost'}:{mqttStatus.listen_port || 1883}
                    </CardDescription>
                  </div>
                </div>
                <Badge variant={mqttStatus.connected ? "default" : "secondary"}>
                  {mqttStatus.connected ? t('devices:connections.builtinMqtt.running') : t('devices:connections.builtinMqtt.stopped')}
                </Badge>
              </div>
            </CardHeader>
            <CardContent className="pb-3">
              <div className="space-y-2 text-sm">
                <div className="flex items-center justify-between">
                  <span className="text-muted-foreground">{t('plugins:llm.status')}:</span>
                  <span className={mqttStatus.connected ? "text-green-600 dark:text-green-400 font-medium" : "text-muted-foreground font-medium"}>
                    {mqttStatus.connected ? t('devices:connections.builtinMqtt.connected') : t('devices:connections.builtinMqtt.disconnected')}
                  </span>
                </div>
                <div className="flex items-center justify-between">
                  <span className="text-muted-foreground">{t('devices:connections.builtinMqtt.deviceCount')}:</span>
                  <span className="font-medium">{getDeviceCount('builtin-mqtt')}</span>
                </div>
              </div>
            </CardContent>
          </Card>
        )}

        {/* Instance Cards */}
        {selectedType.id !== 'builtin-mqtt' && pluginInstances.length === 0 ? (
          <Card className="border-dashed">
            <CardContent className="flex flex-col items-center justify-center py-12">
              <div className={cn("flex items-center justify-center w-16 h-16 rounded-lg mb-4", info.iconBg)}>
                {info.icon}
              </div>
              <h3 className="text-lg font-semibold mb-1">{t('plugins:llm.noInstanceYet', { name: info.name })}</h3>
              <p className="text-sm text-muted-foreground mb-4">
                {t('plugins:llm.configureToStart', { name: info.name })}
              </p>
              <Button onClick={() => {
                setEditingInstance(null)
                setConfigDialogOpen(true)
              }}>
                <Server className="mr-2 h-4 w-4" />
                {t('devices:connections.addConnection')}
              </Button>
            </CardContent>
          </Card>
        ) : selectedType.id !== 'builtin-mqtt' ? (
          <div className="grid gap-4 md:grid-cols-2">
            {pluginInstances.map((instance) => {
              const testResult = testResults[instance.id]
              const isExternalMqtt = instance.plugin_type === 'external-mqtt'
              const isModbus = instance.plugin_type === 'modbus'

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
                          {instance.running && (
                            <Badge variant="default" className="text-xs">{t('plugins:llm.running')}</Badge>
                          )}
                          {instance.enabled && !instance.running && (
                            <Badge variant="outline" className="text-xs">{t('plugins:enabled')}</Badge>
                          )}
                        </div>
                        <CardDescription className="font-mono text-xs">
                          {isExternalMqtt && `${instance.config?.broker}:${instance.config?.port}`}
                          {isModbus && `${instance.config?.host}:${instance.config?.port}`}
                        </CardDescription>
                      </div>
                      <DropdownMenu>
                        <DropdownMenuTrigger asChild>
                          <Button variant="ghost" size="sm" className="h-8 w-8 p-0">
                            <MoreVertical className="h-4 w-4" />
                          </Button>
                        </DropdownMenuTrigger>
                        <DropdownMenuContent align="end">
                          {isExternalMqtt && (
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
                          {isModbus && (
                            <DropdownMenuItem onClick={() => {
                              setEditingInstance(instance)
                              setConfigDialogOpen(true)
                            }}>
                              <Edit className="mr-2 h-4 w-4" />
                              {t('plugins:edit')}
                            </DropdownMenuItem>
                          )}
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
        ) : null}

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
      </>
    )
  }

  return null
}
