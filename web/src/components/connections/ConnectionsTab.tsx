import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import {
  Plus,
  MoreVertical,
  Edit,
  Trash2,
  TestTube,
  ArrowLeft,
  Server,
  Home,
  Network,
  Wifi,
  CheckCircle2,
  Settings,
  RefreshCw,
  Power,
} from 'lucide-react'
import { Card, CardContent, CardDescription, CardHeader, CardTitle, CardFooter } from '@/components/ui/card'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
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
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { cn } from '@/lib/utils'
import { api } from '@/lib/api'
import { toast } from '@/components/ui/use-toast'
import { LoadingState, EmptyState } from '@/components/shared'

type View = 'list' | 'detail'

// Connection type definitions - will be populated by useTranslation
const CONNECTION_ICONS: Record<string, React.ReactNode> = {
  builtinMqtt: <Server className="h-6 w-6" />,
  externalMqtt: <Network className="h-6 w-6" />,
  hass: <Home className="h-6 w-6" />,
  modbus: <Wifi className="h-6 w-6" />,
}

const CONNECTION_ICON_BGS: Record<string, string> = {
  builtinMqtt: 'badge-success',
  externalMqtt: 'badge-info',
  hass: 'bg-purple-100 text-purple-700 dark:bg-purple-900/20 dark:text-purple-400',
  modbus: 'bg-orange-100 text-orange-700 dark:bg-orange-900/20 dark:text-orange-400',
}

const CONNECTION_CONFIG: Record<string, { canAddMultiple: boolean; builtin: boolean }> = {
  builtinMqtt: { canAddMultiple: false, builtin: true },
  externalMqtt: { canAddMultiple: true, builtin: false },
  hass: { canAddMultiple: false, builtin: false },
  modbus: { canAddMultiple: true, builtin: false },
}

export function ConnectionsTab() {
  const { t } = useTranslation(['common', 'devices'])
  const [view, setView] = useState<View>('list')
  const [loading, setLoading] = useState(true)
  const [selectedType, setSelectedType] = useState<string | null>(null)

  // Data states
  const [mqttStatus, setMqttStatus] = useState<any>(null)
  const [externalBrokers, setExternalBrokers] = useState<any[]>([])
  const [hassStatus, setHassStatus] = useState<any>(null)
  const [modbusAdapters, setModbusAdapters] = useState<any[]>([])
  const [devices, setDevices] = useState<any[]>([])

  // Dialog states
  const [configDialogOpen, setConfigDialogOpen] = useState(false)
  const [editingItem, setEditingItem] = useState<any>(null)
  const [saving, setSaving] = useState(false)

  // View devices dialog
  const [devicesDialogOpen, setDevicesDialogOpen] = useState(false)
  const [connectionDevices, setConnectionDevices] = useState<any[]>([])
  const [connectionName, setConnectionName] = useState('')

  useEffect(() => {
    loadData()
  }, [])

  const loadData = async () => {
    setLoading(true)
    try {
      // Load all connection data
      const [mqttResult, brokersResult, hassResult, devicesResult] = await Promise.allSettled([
        api.getMqttStatus(),
        api.getBrokers(),
        api.getHassDiscoveryStatus(),
        api.getDevices(),
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

      // Load Modbus adapters from plugins API
      try {
        const adaptersData = await api.listDeviceAdapters()
        const modbus = adaptersData.adapters?.filter((a: any) => a.adapter_type === 'modbus') || []
        setModbusAdapters(modbus)
      } catch {
        setModbusAdapters([])
      }
    } catch (error) {
      console.error('Failed to load connections data:', error)
    } finally {
      setLoading(false)
    }
  }

  // Get device count for a connection
  const getDeviceCount = (type: string, id?: string) => {
    if (type === 'builtinMqtt') {
      return devices.filter((d: any) =>
        !d.plugin_id || d.plugin_id === 'internal-mqtt' || d.plugin_id === 'builtin'
      ).length
    } else if (type === 'externalMqtt' && id) {
      return devices.filter((d: any) => d.plugin_id === id).length
    } else if (type === 'hass') {
      return devices.filter((d: any) => d.plugin_id === 'hass-discovery').length
    } else if (type === 'modbus' && id) {
      return devices.filter((d: any) => d.plugin_id === id).length
    }
    return 0
  }

  // Get status for a connection type
  const getConnectionStatus = (type: string) => {
    if (type === 'builtinMqtt') {
      return mqttStatus?.connected || false
    } else if (type === 'externalMqtt') {
      return externalBrokers.some((b) => b.connected)
    } else if (type === 'hass') {
      return hassStatus?.hass_discovery?.enabled || false
    } else if (type === 'modbus') {
      return modbusAdapters.some((a) => a.running)
    }
    return false
  }

  const handleViewDevices = async (type: string, id?: string) => {
    let filteredDevices: any[] = []

    if (type === 'builtinMqtt') {
      filteredDevices = devices.filter((d: any) =>
        !d.plugin_id || d.plugin_id === 'internal-mqtt' || d.plugin_id === 'builtin'
      )
      setConnectionName(t('devices:connections.builtinMqtt.title'))
    } else if (type === 'externalMqtt' && id) {
      filteredDevices = devices.filter((d: any) => d.plugin_id === id)
      const broker = externalBrokers.find((b) => b.id === id)
      setConnectionName(broker?.name || t('devices:connections.externalMqtt.name'))
    } else if (type === 'hass') {
      filteredDevices = devices.filter((d: any) => d.plugin_id === 'hass-discovery')
      setConnectionName(t('devices:connections.hass.name'))
    } else if (type === 'modbus' && id) {
      filteredDevices = devices.filter((d: any) => d.plugin_id === id)
      const adapter = modbusAdapters.find((a) => a.id === id)
      setConnectionName(adapter?.name || t('devices:connections.modbus.name'))
    }

    setConnectionDevices(filteredDevices)
    setDevicesDialogOpen(true)
  }

  const handleDelete = async (type: string, id: string) => {
    if (type === 'externalMqtt') {
      if (!confirm(t('devices:connections.externalMqtt.deleteConfirm'))) return
      try {
        await api.deleteBroker(id)
        await loadData()
        toast({ title: t('devices:connections.externalMqtt.deleted'), description: t('devices:connections.externalMqtt.deletedDesc') })
      } catch (error) {
        toast({ title: t('devices:connections.externalMqtt.deleteFailed'), description: (error as Error).message, variant: 'destructive' })
      }
    } else if (type === 'modbus') {
      if (!confirm(t('devices:connections.modbus.deleteConfirm'))) return
      try {
        await api.unregisterPlugin(id)
        await loadData()
        toast({ title: t('devices:connections.modbus.deleted'), description: t('devices:connections.modbus.deletedDesc') })
      } catch (error) {
        toast({ title: t('devices:connections.modbus.deleteFailed'), description: (error as Error).message, variant: 'destructive' })
      }
    } else if (type === 'hass') {
      toast({
        title: t('common:hint'),
        description: t('devices:connections.hass.configureInSettings'),
      })
    }
  }

  const handleTest = async (type: string, id?: string) => {
    if (type === 'externalMqtt' && id) {
      try {
        const result = await api.testBroker(id)
        if (result.success) {
          toast({ title: t('devices:connections.externalMqtt.connectionSuccess'), description: result.message })
        } else {
          toast({ title: t('devices:connections.externalMqtt.connectionFailed'), description: result.message, variant: 'destructive' })
        }
        await loadData()
      } catch (error) {
        toast({ title: t('devices:connections.externalMqtt.testFailed'), description: (error as Error).message, variant: 'destructive' })
      }
    } else {
      toast({ title: t('common:hint'), description: t('devices:connections.externalMqtt.testNotSupported') })
    }
  }

  const handleToggle = async (type: string, id?: string) => {
    if (type === 'externalMqtt' && id) {
      try {
        const broker = externalBrokers.find(b => b.id === id)
        if (!broker) return

        await api.updateBroker(id, {
          name: broker.name,
          broker: broker.broker,
          port: broker.port,
          tls: broker.tls,
          username: broker.username,
          password: broker.password || '',
          enabled: !broker.enabled,
        })

        const enabled = !broker.enabled
        toast({
          title: enabled ? t('devices:connections.externalMqtt.enabled') : t('devices:connections.externalMqtt.disabled'),
          description: `MQTT Broker ${enabled ? t('devices:connections.externalMqtt.enabled') : t('devices:connections.externalMqtt.disabled')}`
        })
        await loadData()
      } catch (error) {
        toast({ title: t('common:actionFailed'), description: (error as Error).message, variant: 'destructive' })
      }
    } else {
      toast({ title: t('common:hint'), description: t('devices:connections.externalMqtt.toggleNotSupported') })
    }
  }

  if (loading) {
    return <LoadingState text={t('common:loading')} />
  }

  // Connection types with i18n
  const connectionTypes = ['builtinMqtt', 'externalMqtt', 'hass', 'modbus'] as const

  // ========== LIST VIEW ==========
  if (view === 'list') {
    return (
      <>
        {/* Header */}
        <div className="flex items-center justify-between mb-4">
          <div>
            <h2 className="text-2xl font-bold tracking-tight">{t('devices:deviceAdapters')}</h2>
            <p className="text-muted-foreground text-sm">
              {t('devices:connections.description')}
            </p>
          </div>
        </div>

        {/* Connection Type Cards */}
        <div className="grid gap-4 md:grid-cols-2 lg:grid-cols-4">
          {connectionTypes.map((typeId) => {
            const isActive = getConnectionStatus(typeId)
            const deviceCount = getDeviceCount(typeId)
            const icon = CONNECTION_ICONS[typeId]
            const iconBg = CONNECTION_ICON_BGS[typeId]

            return (
              <Card
                key={typeId}
                className={cn(
                  "cursor-pointer transition-all duration-200 hover:shadow-md",
                  isActive && "border-green-500 border-2"
                )}
                onClick={() => {
                  setSelectedType(typeId)
                  setView('detail')
                }}
              >
                <CardHeader className="pb-3">
                  <div className={cn("flex items-center justify-center w-12 h-12 rounded-lg", iconBg)}>
                    {icon}
                  </div>
                  <CardTitle className="text-base mt-3">{t(`devices:connections.${typeId}.name`)}</CardTitle>
                  <CardDescription className="mt-1 text-xs">
                    {t(`devices:connections.${typeId}.description`)}
                  </CardDescription>
                </CardHeader>
                <CardContent className="text-sm">
                  <div className="flex justify-between items-center">
                    <span className="text-muted-foreground">{t('devices:connections.status')}:</span>
                    <span className={isActive ? "text-success font-medium" : "text-muted-foreground font-medium"}>
                      {isActive ? t('devices:connections.running') : t('devices:connections.notConfigured')}
                    </span>
                  </div>
                  <div className="flex justify-between items-center mt-2">
                    <span className="text-muted-foreground">{t('devices:connections.devices')}:</span>
                    <span className="font-medium">{deviceCount}</span>
                  </div>
                </CardContent>
              </Card>
            )
          })}
        </div>

        {/* View Devices Dialog */}
        <DevicesDialog
          open={devicesDialogOpen}
          onClose={() => setDevicesDialogOpen(false)}
          devices={connectionDevices}
          connectionName={connectionName}
        />
      </>
    )
  }

  // ========== DETAIL VIEW ==========
  if (view === 'detail' && selectedType) {
    const icon = CONNECTION_ICONS[selectedType]
    const iconBg = CONNECTION_ICON_BGS[selectedType]
    const config = CONNECTION_CONFIG[selectedType]

    return (
      <>
        {/* Header with back button */}
        <div className="flex items-center gap-4 mb-4">
          <Button variant="ghost" size="sm" onClick={() => setView('list')} className="gap-1">
            <ArrowLeft className="h-4 w-4" />
            {t('devices:connections.back')}
          </Button>
          <div className="flex items-center gap-3">
            <div className={cn("flex items-center justify-center w-10 h-10 rounded-lg", iconBg)}>
              {icon}
            </div>
            <div>
              <h2 className="text-2xl font-bold">{t(`devices:connections.${selectedType}.name`)}</h2>
              <p className="text-sm text-muted-foreground">{t(`devices:connections.${selectedType}.description`)}</p>
            </div>
          </div>
          {config.canAddMultiple && (
            <div className="ml-auto">
              <Button onClick={() => {
                setEditingItem(null)
                setConfigDialogOpen(true)
              }}>
                <Plus className="mr-2 h-4 w-4" />
                {t('devices:connections.addConnection')}
              </Button>
            </div>
          )}
        </div>

        {/* Builtin MQTT Detail */}
        {selectedType === 'builtinMqtt' && (
          <BuiltinMqttCard
            mqttStatus={mqttStatus}
            deviceCount={getDeviceCount('builtinMqtt')}
            onViewDevices={() => handleViewDevices('builtinMqtt')}
            onRefresh={loadData}
          />
        )}

        {/* External MQTT Detail */}
        {selectedType === 'externalMqtt' && (
          <div className="space-y-4">
            {externalBrokers.length === 0 ? (
              <Card className="border-dashed">
                <CardContent className="flex flex-col items-center justify-center py-12">
                  <div className={cn("flex items-center justify-center w-16 h-16 rounded-lg mb-4", iconBg)}>
                    {icon}
                  </div>
                  <h3 className="text-lg font-semibold mb-1">{t('devices:connections.externalMqtt.noConnections')}</h3>
                  <p className="text-sm text-muted-foreground mb-4">
                    {t('devices:connections.externalMqtt.addDesc')}
                  </p>
                  <Button onClick={() => {
                    setEditingItem(null)
                    setConfigDialogOpen(true)
                  }}>
                    <Plus className="mr-2 h-4 w-4" />
                    {t('devices:connections.externalMqtt.addBroker')}
                  </Button>
                </CardContent>
              </Card>
            ) : (
              externalBrokers.map((broker) => (
                <ExternalBrokerCard
                  key={broker.id}
                  broker={broker}
                  deviceCount={getDeviceCount('externalMqtt', broker.id)}
                  onEdit={() => {
                    setEditingItem(broker)
                    setConfigDialogOpen(true)
                  }}
                  onDelete={() => handleDelete('externalMqtt', broker.id)}
                  onTest={() => handleTest('externalMqtt', broker.id)}
                  onToggle={() => handleToggle('externalMqtt', broker.id)}
                  onViewDevices={() => handleViewDevices('externalMqtt', broker.id)}
                />
              ))
            )}
          </div>
        )}

        {/* Home Assistant Detail */}
        {selectedType === 'hass' && (
          <HassCard
            hassStatus={hassStatus}
            deviceCount={getDeviceCount('hass')}
            onViewDevices={() => handleViewDevices('hass')}
            onRefresh={loadData}
            onConfigure={() => {
              setEditingItem({ type: 'hass' })
              setConfigDialogOpen(true)
            }}
          />
        )}

        {/* Modbus Detail */}
        {selectedType === 'modbus' && (
          <div className="space-y-4">
            {modbusAdapters.length === 0 ? (
              <Card className="border-dashed">
                <CardContent className="flex flex-col items-center justify-center py-12">
                  <div className={cn("flex items-center justify-center w-16 h-16 rounded-lg mb-4", iconBg)}>
                    {icon}
                  </div>
                  <h3 className="text-lg font-semibold mb-1">{t('devices:connections.modbus.noConnections')}</h3>
                  <p className="text-sm text-muted-foreground mb-4">
                    {t('devices:connections.modbus.addDesc')}
                  </p>
                  <Button onClick={() => {
                    setEditingItem(null)
                    setConfigDialogOpen(true)
                  }}>
                    <Plus className="mr-2 h-4 w-4" />
                    {t('devices:connections.modbus.addConnection')}
                  </Button>
                </CardContent>
              </Card>
            ) : (
              modbusAdapters.map((adapter) => (
                <ModbusCard
                  key={adapter.id}
                  adapter={adapter}
                  deviceCount={getDeviceCount('modbus', adapter.id)}
                  onEdit={() => {
                    setEditingItem(adapter)
                    setConfigDialogOpen(true)
                  }}
                  onDelete={() => handleDelete('modbus', adapter.id)}
                  onViewDevices={() => handleViewDevices('modbus', adapter.id)}
                />
              ))
            )}
          </div>
        )}

        {/* Config Dialog */}
        {configDialogOpen && (
          <ConnectionConfigDialog
            open={configDialogOpen}
            type={selectedType}
            editing={editingItem}
            onClose={() => {
              setConfigDialogOpen(false)
              setEditingItem(null)
            }}
            onSave={async () => {
              setSaving(true)
              try {
                await loadData()
                setConfigDialogOpen(false)
                setEditingItem(null)
                toast({ title: '保存成功', description: '连接配置已保存' })
              } catch (error) {
                toast({ title: '保存失败', description: (error as Error).message, variant: 'destructive' })
              } finally {
                setSaving(false)
              }
            }}
            saving={saving}
          />
        )}

        {/* View Devices Dialog */}
        <DevicesDialog
          open={devicesDialogOpen}
          onClose={() => setDevicesDialogOpen(false)}
          devices={connectionDevices}
          connectionName={connectionName}
        />
      </>
    )
  }

  return null
}

// Builtin MQTT Card
function BuiltinMqttCard({ mqttStatus, deviceCount, onViewDevices, onRefresh }: {
  mqttStatus: any
  deviceCount: number
  onViewDevices: () => void
  onRefresh: () => void
}) {
  const { t } = useTranslation(['devices'])
  const connected = mqttStatus?.connected || false

  return (
    <Card className={cn("transition-all duration-200", connected && "border-green-500")}>
      <CardHeader className="pb-3">
        <div className="flex items-start justify-between">
          <div className="flex items-center gap-3">
            <div className="p-2 rounded-lg badge-success">
              <Server className="h-5 w-5" />
            </div>
            <div>
              <CardTitle className="text-base">{t('devices:connections.builtinMqtt.title')}</CardTitle>
              <CardDescription className="text-xs">
                {mqttStatus?.server_ip}:{mqttStatus?.listen_port || 1883}
              </CardDescription>
            </div>
          </div>
          <Badge variant={connected ? "default" : "secondary"}>
            {connected ? t('devices:connections.builtinMqtt.running') : t('devices:connections.builtinMqtt.stopped')}
          </Badge>
        </div>
      </CardHeader>
      <CardContent className="pb-3">
        <div className="space-y-2 text-sm">
          <div className="flex items-center justify-between">
            <span className="text-muted-foreground">{t('devices:connections.status')}:</span>
            <span className={connected ? "text-success font-medium" : "text-muted-foreground font-medium"}>
              {connected ? t('devices:connections.builtinMqtt.connected') : t('devices:connections.builtinMqtt.disconnected')}
            </span>
          </div>
          <div className="flex items-center justify-between">
            <span className="text-muted-foreground">{t('devices:connections.builtinMqtt.deviceCount')}:</span>
            <span className="font-medium">{deviceCount}</span>
          </div>
        </div>
      </CardContent>
      <CardFooter className="pt-3 border-t justify-between">
        <Button variant="outline" size="sm" onClick={onViewDevices}>
          <Wifi className="mr-2 h-4 w-4" />
          {t('devices:connections.builtinMqtt.viewDevices')}
        </Button>
        <Button variant="ghost" size="sm" onClick={onRefresh}>
          {t('devices:connections.builtinMqtt.refreshStatus')}
        </Button>
      </CardFooter>
    </Card>
  )
}

// External Broker Card
function ExternalBrokerCard({ broker, deviceCount, onEdit, onDelete, onTest, onToggle, onViewDevices }: {
  broker: any
  deviceCount: number
  onEdit: () => void
  onDelete: () => void
  onTest: () => void
  onToggle: () => void
  onViewDevices: () => void
}) {
  const { t } = useTranslation(['devices'])
  const connected = broker.connected || false
  const enabled = broker.enabled || false

  return (
    <Card className={cn("transition-all duration-200", connected && "border-green-500")}>
      <CardHeader className="pb-3">
        <div className="flex items-start justify-between">
          <div className="flex-1">
            <div className="flex items-center gap-2 mb-1">
              <CardTitle className="text-base">{broker.name}</CardTitle>
              <Badge variant={connected ? "default" : "secondary"} className="text-xs">
                {connected ? t('devices:connections.externalMqtt.connected') : t('devices:connections.externalMqtt.disconnected')}
              </Badge>
              <Badge variant={enabled ? "outline" : "secondary"} className="text-xs">
                {enabled ? t('devices:connections.externalMqtt.enabled') : t('devices:connections.externalMqtt.disabled')}
              </Badge>
            </div>
            <CardDescription className="font-mono text-xs">
              {broker.broker}:{broker.port}
            </CardDescription>
          </div>
          <DropdownMenu>
            <DropdownMenuTrigger asChild>
              <Button variant="ghost" size="sm" className="h-8 w-8 p-0">
                <MoreVertical className="h-4 w-4" />
              </Button>
            </DropdownMenuTrigger>
            <DropdownMenuContent align="end">
              <DropdownMenuItem onClick={onToggle}>
                <Power className="mr-2 h-4 w-4" />
                {enabled ? t('devices:connections.externalMqtt.disable') : t('devices:connections.externalMqtt.enable')}
              </DropdownMenuItem>
              <DropdownMenuItem onClick={onEdit}>
                <Edit className="mr-2 h-4 w-4" />
                {t('devices:connections.externalMqtt.edit')}
              </DropdownMenuItem>
              <DropdownMenuItem onClick={onTest}>
                <TestTube className="mr-2 h-4 w-4" />
                {t('devices:connections.externalMqtt.testConnection')}
              </DropdownMenuItem>
              <DropdownMenuSeparator />
              <DropdownMenuItem onClick={onDelete} className="text-destructive focus:text-destructive">
                <Trash2 className="mr-2 h-4 w-4" />
                {t('devices:connections.externalMqtt.delete')}
              </DropdownMenuItem>
            </DropdownMenuContent>
          </DropdownMenu>
        </div>
      </CardHeader>
      <CardContent className="pb-3">
        <div className="space-y-2 text-sm">
          <div className="flex items-center justify-between">
            <span className="text-muted-foreground">{t('devices:connections.externalMqtt.deviceCount')}:</span>
            <span className="font-medium">{deviceCount}</span>
          </div>
          {broker.tls && (
            <div className="flex items-center gap-2">
              <CheckCircle2 className="h-3 w-3 text-success" />
              <span className="text-xs text-muted-foreground">{t('devices:connections.externalMqtt.tlsEnabled')}</span>
            </div>
          )}
          {broker.username && (
            <div className="flex items-center justify-between">
              <span className="text-muted-foreground">{t('devices:connections.externalMqtt.auth')}:</span>
              <span className="text-xs">{t('devices:connections.externalMqtt.configured')}</span>
            </div>
          )}
          {broker.last_error && (
            <div className="text-xs text-destructive">
              {broker.last_error}
            </div>
          )}
        </div>
      </CardContent>
      <CardFooter className="pt-3 border-t justify-between">
        <Button variant="outline" size="sm" onClick={onViewDevices}>
          <Wifi className="mr-2 h-4 w-4" />
          {t('devices:connections.externalMqtt.viewDevices')}
        </Button>
      </CardFooter>
    </Card>
  )
}

// Home Assistant Card
function HassCard({ hassStatus, deviceCount, onViewDevices, onRefresh, onConfigure }: {
  hassStatus: any
  deviceCount: number
  onViewDevices: () => void
  onRefresh: () => void
  onConfigure: () => void
}) {
  const { t } = useTranslation(['devices'])
  const connected = hassStatus?.hass_integration?.connected || false
  const configured = hassStatus?.hass_integration?.url || false

  return (
    <Card className={cn("transition-all duration-200", connected && "border-green-500")}>
      <CardHeader className="pb-3">
        <div className="flex items-start justify-between">
          <div className="flex items-center gap-3">
            <div className="p-2 rounded-lg bg-purple-100 text-purple-700 dark:bg-purple-900/20 dark:text-purple-400">
              <Home className="h-5 w-5" />
            </div>
            <div>
              <CardTitle className="text-base">{t('devices:connections.hass.title')}</CardTitle>
              <CardDescription className="text-xs">
                {hassStatus?.hass_integration?.url || t('devices:connections.hass.notConfigured')}
              </CardDescription>
            </div>
          </div>
          <Badge variant={connected ? "default" : "secondary"}>
            {connected ? t('devices:connections.hass.connected') : configured ? t('devices:connections.hass.disconnected') : t('devices:connections.hass.notConfigured')}
          </Badge>
        </div>
      </CardHeader>
      <CardContent className="pb-3">
        <div className="space-y-2 text-sm">
          <div className="flex items-center justify-between">
            <span className="text-muted-foreground">{t('devices:connections.hass.connectionStatus')}:</span>
            <span className={connected ? "text-success font-medium" : "text-muted-foreground font-medium"}>
              {connected ? t('devices:connections.hass.connected') : t('devices:connections.hass.disconnected')}
            </span>
          </div>
          <div className="flex items-center justify-between">
            <span className="text-muted-foreground">{t('devices:connections.hass.deviceCount')}:</span>
            <span className="font-medium">{deviceCount}</span>
          </div>
          <p className="text-xs text-muted-foreground mt-2">
            {t('devices:connections.hass.description1')}
          </p>
          <p className="text-xs text-muted-foreground">
            {t('devices:connections.hass.description2')}
          </p>
        </div>
      </CardContent>
      <CardFooter className="pt-3 border-t flex flex-wrap gap-2">
        <Button variant="outline" size="sm" onClick={onViewDevices}>
          <Wifi className="mr-2 h-4 w-4" />
          {t('devices:connections.hass.viewDevices')}
        </Button>
        <div className="flex gap-2 ml-auto">
          <Button variant="ghost" size="sm" onClick={onRefresh}>
            <RefreshCw className="mr-2 h-4 w-4" />
            {t('devices:connections.hass.refresh')}
          </Button>
          <Button variant="ghost" size="sm" onClick={onConfigure}>
            <Settings className="mr-2 h-4 w-4" />
            {t('devices:connections.hass.configure')}
          </Button>
        </div>
      </CardFooter>
    </Card>
  )
}

// Modbus Card
function ModbusCard({ adapter, deviceCount, onEdit, onDelete, onViewDevices }: {
  adapter: any
  deviceCount: number
  onEdit: () => void
  onDelete: () => void
  onViewDevices: () => void
}) {
  const { t } = useTranslation(['devices'])
  const running = adapter.running || false

  return (
    <Card className={cn("transition-all duration-200", running && "border-green-500")}>
      <CardHeader className="pb-3">
        <div className="flex items-start justify-between">
          <div className="flex-1">
            <div className="flex items-center gap-2 mb-1">
              <CardTitle className="text-base">{adapter.name}</CardTitle>
              <Badge variant={running ? "default" : "secondary"} className="text-xs">
                {running ? t('devices:connections.modbus.running') : t('devices:connections.modbus.stopped')}
              </Badge>
            </div>
            <CardDescription className="font-mono text-xs">
              {adapter.config?.host}:{adapter.config?.port}
            </CardDescription>
          </div>
          <DropdownMenu>
            <DropdownMenuTrigger asChild>
              <Button variant="ghost" size="sm" className="h-8 w-8 p-0">
                <MoreVertical className="h-4 w-4" />
              </Button>
            </DropdownMenuTrigger>
            <DropdownMenuContent align="end">
              <DropdownMenuItem onClick={onEdit}>
                <Edit className="mr-2 h-4 w-4" />
                {t('devices:connections.modbus.edit')}
              </DropdownMenuItem>
              <DropdownMenuSeparator />
              <DropdownMenuItem onClick={onDelete} className="text-destructive focus:text-destructive">
                <Trash2 className="mr-2 h-4 w-4" />
                {t('devices:connections.modbus.delete')}
              </DropdownMenuItem>
            </DropdownMenuContent>
          </DropdownMenu>
        </div>
      </CardHeader>
      <CardContent className="pb-3">
        <div className="space-y-2 text-sm">
          <div className="flex items-center justify-between">
            <span className="text-muted-foreground">{t('devices:connections.modbus.slaveId')}:</span>
            <span className="font-medium">{adapter.config?.slave_id}</span>
          </div>
          <div className="flex items-center justify-between">
            <span className="text-muted-foreground">{t('devices:connections.modbus.deviceCount')}:</span>
            <span className="font-medium">{deviceCount}</span>
          </div>
        </div>
      </CardContent>
      <CardFooter className="pt-3 border-t">
        <Button variant="outline" size="sm" onClick={onViewDevices}>
          <Wifi className="mr-2 h-4 w-4" />
          {t('devices:connections.modbus.viewDevices')}
        </Button>
      </CardFooter>
    </Card>
  )
}

// Connection Config Dialog
function ConnectionConfigDialog({ open, type, editing, onClose, onSave, saving }: {
  open: boolean
  type: string
  editing: any
  onClose: () => void
  onSave: () => Promise<void>
  saving: boolean
}) {
  const { t } = useTranslation(['devices', 'common'])
  const [isSubmitting, setIsSubmitting] = useState(false)
  const [name, setName] = useState(editing?.name || '')
  const [broker, setBroker] = useState(editing?.broker || '')
  const [port, setPort] = useState(editing?.port || 1883)
  const [username, setUsername] = useState(editing?.username || '')
  const [password, setPassword] = useState('')
  const [tls, setTls] = useState(editing?.tls || false)

  // TLS Certificate fields
  const [caCert, setCaCert] = useState(editing?.ca_cert || '')
  const [clientCert, setClientCert] = useState(editing?.client_cert || '')
  const [clientKey, setClientKey] = useState(editing?.client_key || '')

  // Modbus fields
  const [modbusHost, setModbusHost] = useState(editing?.config?.host || '192.168.1.100')
  const [modbusPort, setModbusPort] = useState(editing?.config?.port || 502)
  const [slaveId, setSlaveId] = useState(editing?.config?.slave_id || 1)

  // HASS fields
  const [hassUrl, setHassUrl] = useState(editing?.url || 'http://homeassistant.local:8123')
  const [hassToken, setHassToken] = useState('')
  const [hassVerifySsl, setHassVerifySsl] = useState(editing?.verify_ssl !== false)
  const [hassAutoImport, setHassAutoImport] = useState(editing?.auto_import || false)

  useEffect(() => {
    if (open) {
      if (type === 'externalMqtt') {
        setName(editing?.name || '')
        setBroker(editing?.broker || '')
        setPort(editing?.port || 1883)
        setUsername(editing?.username || '')
        setPassword('')
        setTls(editing?.tls || false)
        setCaCert(editing?.ca_cert || '')
        setClientCert(editing?.client_cert || '')
        setClientKey(editing?.client_key || '')
      } else if (type === 'modbus') {
        setName(editing?.name || '')
        setModbusHost(editing?.config?.host || '192.168.1.100')
        setModbusPort(editing?.config?.port || 502)
        setSlaveId(editing?.config?.slave_id || 1)
      } else if (type === 'hass') {
        setName(editing?.name || 'Home Assistant')
        setHassUrl(editing?.url || 'http://homeassistant.local:8123')
        setHassToken('') // Never pre-fill token for security
        setHassVerifySsl(editing?.verify_ssl !== false)
        setHassAutoImport(editing?.auto_import || false)
      }
    }
  }, [open, editing, type])

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault()

    setIsSubmitting(true)
    try {
      if (type === 'externalMqtt') {
        const brokerData: any = {
          name: name.trim(),
          broker: broker.trim(),
          port,
          username: username || undefined,
          password: password || undefined,
          tls,
        }

        // Add certificate fields only if TLS is enabled
        if (tls) {
          if (caCert?.trim()) brokerData.ca_cert = caCert.trim()
          if (clientCert?.trim()) brokerData.client_cert = clientCert.trim()
          if (clientKey?.trim()) brokerData.client_key = clientKey.trim()
        }

        if (editing) {
          await api.updateBroker(editing.id, {
            ...brokerData,
            enabled: editing.enabled,
          })
        } else {
          await api.createBroker({
            ...brokerData,
            enabled: true,
          })
        }
      } else if (type === 'modbus') {
        const adapterData = {
          id: editing?.id || `modbus-${Date.now()}`,
          name: name.trim(),
          adapter_type: 'modbus',
          config: {
            host: modbusHost,
            port: modbusPort,
            slave_id: slaveId,
          },
          auto_start: true,
          enabled: true,
        }

        if (editing) {
          await api.updatePluginConfig(editing.id, adapterData.config)
        } else {
          await api.registerDeviceAdapter(adapterData)
        }
      } else if (type === 'hass') {
        // Connect to HASS using the integration API
        await api.connectHass({
          url: hassUrl.trim(),
          token: hassToken.trim(),
          verify_ssl: hassVerifySsl,
          auto_import: hassAutoImport,
        })
      }
      await onSave()
    } catch (error) {
      throw error
    } finally {
      setIsSubmitting(false)
    }
  }

  if (!open) return null

  const icon = CONNECTION_ICONS[type]
  const iconBg = CONNECTION_ICON_BGS[type]

  return (
    <Dialog open={open} onOpenChange={onClose}>
      <DialogContent className="max-w-md">
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2">
            <div className={cn("p-2 rounded-lg", iconBg)}>
              {icon}
            </div>
            {editing ? t('devices:connectionDialog.editTitle') : t('devices:connectionDialog.addTitle')} {t(`devices:connections.${type}.name`)}
          </DialogTitle>
          <DialogDescription>
            {editing ? t('devices:connectionDialog.editDesc') : t('devices:connectionDialog.addDesc')} {t(`devices:connections.${type}.name`)} {t('devices:connectionDialog.connectionInfo')}
          </DialogDescription>
        </DialogHeader>

        <form onSubmit={handleSubmit} className="space-y-4">
          <div>
            <Label htmlFor="name">{t('devices:connectionDialog.displayNameRequired')}</Label>
            <Input
              id="name"
              value={name}
              onChange={(e) => setName(e.target.value)}
              placeholder={type === 'externalMqtt' ? t('devices:connectionDialog.displayNamePlaceholderMqtt') : t('devices:connectionDialog.displayNamePlaceholderModbus')}
              required
            />
          </div>

          {type === 'externalMqtt' && (
            <>
              <div>
                <Label htmlFor="broker">{t('devices:connectionDialog.brokerAddress')}</Label>
                <Input
                  id="broker"
                  value={broker}
                  onChange={(e) => setBroker(e.target.value)}
                  placeholder={t('devices:connectionDialog.brokerPlaceholder')}
                  required
                />
              </div>
              <div>
                <Label htmlFor="port">{t('devices:connectionDialog.port')}</Label>
                <Input
                  id="port"
                  type="number"
                  value={port}
                  onChange={(e) => setPort(parseInt(e.target.value) || 1883)}
                />
              </div>
              <div>
                <Label htmlFor="username">{t('devices:connectionDialog.username')}</Label>
                <Input
                  id="username"
                  value={username}
                  onChange={(e) => setUsername(e.target.value)}
                  placeholder={t('devices:connectionDialog.usernamePlaceholder')}
                />
              </div>
              <div>
                <Label htmlFor="password">{t('devices:connectionDialog.password')}</Label>
                <Input
                  id="password"
                  type="password"
                  value={password}
                  onChange={(e) => setPassword(e.target.value)}
                  placeholder={editing ? t('devices:connectionDialog.passwordKeepHint') : t('devices:connectionDialog.passwordPlaceholder')}
                />
              </div>
              <div className="flex items-center gap-2">
                <input
                  type="checkbox"
                  id="tls"
                  checked={tls}
                  onChange={(e) => {
                    setTls(e.target.checked)
                    // Auto-switch port if using default
                    if ((e.target.checked && port === 1883) || (!e.target.checked && port === 8883)) {
                      setPort(e.target.checked ? 8883 : 1883)
                    }
                  }}
                  className="h-4 w-4"
                />
                <Label htmlFor="tls" className="cursor-pointer">{t('devices:connectionDialog.enableTls')}</Label>
              </div>

              {/* TLS Certificate Configuration */}
              {tls && (
                <div className="space-y-3 p-3 border rounded-md bg-muted/30">
                  <div className="text-xs font-medium text-info dark:text-blue-400">
                    🔒 {t('devices:connectionDialog.tlsCertConfig')}
                  </div>
                  <div>
                    <Label htmlFor="caCert" className="text-xs">{t('devices:connectionDialog.caCert')}</Label>
                    <textarea
                      id="caCert"
                      value={caCert}
                      onChange={(e) => setCaCert(e.target.value)}
                      placeholder="-----BEGIN CERTIFICATE-----"
                      className="w-full min-h-[60px] px-2 py-1 text-xs font-mono border rounded-md resize-none bg-background"
                    />
                  </div>
                  <div>
                    <Label htmlFor="clientCert" className="text-xs">{t('devices:connectionDialog.clientCert')}</Label>
                    <textarea
                      id="clientCert"
                      value={clientCert}
                      onChange={(e) => setClientCert(e.target.value)}
                      placeholder="-----BEGIN CERTIFICATE-----"
                      className="w-full min-h-[60px] px-2 py-1 text-xs font-mono border rounded-md resize-none bg-background"
                    />
                  </div>
                  <div>
                    <Label htmlFor="clientKey" className="text-xs">{t('devices:connectionDialog.clientKey')}</Label>
                    <textarea
                      id="clientKey"
                      value={clientKey}
                      onChange={(e) => setClientKey(e.target.value)}
                      placeholder="-----BEGIN PRIVATE KEY-----"
                      className="w-full min-h-[60px] px-2 py-1 text-xs font-mono border rounded-md resize-none bg-background"
                    />
                  </div>
                </div>
              )}
            </>
          )}

          {type === 'modbus' && (
            <>
              <div>
                <Label htmlFor="modbusHost">{t('devices:connectionDialog.hostAddress')}</Label>
                <Input
                  id="modbusHost"
                  value={modbusHost}
                  onChange={(e) => setModbusHost(e.target.value)}
                  placeholder={t('devices:connectionDialog.hostPlaceholder')}
                  required
                />
              </div>
              <div>
                <Label htmlFor="modbusPort">{t('devices:connectionDialog.modbusPort')}</Label>
                <Input
                  id="modbusPort"
                  type="number"
                  value={modbusPort}
                  onChange={(e) => setModbusPort(parseInt(e.target.value) || 502)}
                />
              </div>
              <div>
                <Label htmlFor="slaveId">{t('devices:connectionDialog.modbusSlaveId')}</Label>
                <Input
                  id="slaveId"
                  type="number"
                  value={slaveId}
                  onChange={(e) => setSlaveId(parseInt(e.target.value) || 1)}
                />
              </div>
            </>
          )}

          {type === 'hass' && (
            <>
              <div>
                <Label htmlFor="hassUrl">{t('devices:connectionDialog.hassUrl')}</Label>
                <Input
                  id="hassUrl"
                  type="url"
                  value={hassUrl}
                  onChange={(e) => setHassUrl(e.target.value)}
                  placeholder={t('devices:connectionDialog.hassUrlPlaceholder')}
                  required
                />
                <p className="text-xs text-muted-foreground mt-1">
                  {t('devices:connectionDialog.hassUrlHint')}
                </p>
              </div>
              <div>
                <Label htmlFor="hassToken">{t('devices:connectionDialog.hassToken')}</Label>
                <Input
                  id="hassToken"
                  type="password"
                  value={hassToken}
                  onChange={(e) => setHassToken(e.target.value)}
                  placeholder={t('devices:connectionDialog.hassTokenPlaceholder')}
                  required
                />
                <p className="text-xs text-muted-foreground mt-1">
                  {t('devices:connectionDialog.hassTokenHint')}
                </p>
              </div>
              <div className="flex items-center gap-2">
                <input
                  type="checkbox"
                  id="hassVerifySsl"
                  checked={hassVerifySsl}
                  onChange={(e) => setHassVerifySsl(e.target.checked)}
                  className="h-4 w-4"
                />
                <Label htmlFor="hassVerifySsl" className="cursor-pointer">{t('devices:connectionDialog.verifySsl')}</Label>
              </div>
              <div className="flex items-center gap-2">
                <input
                  type="checkbox"
                  id="hassAutoImport"
                  checked={hassAutoImport}
                  onChange={(e) => setHassAutoImport(e.target.checked)}
                  className="h-4 w-4"
                />
                <Label htmlFor="hassAutoImport" className="cursor-pointer">{t('devices:connectionDialog.autoImport')}</Label>
              </div>
            </>
          )}

          <DialogFooter>
            <Button type="button" variant="outline" onClick={onClose} disabled={saving || isSubmitting}>
              {t('devices:connectionDialog.cancel')}
            </Button>
            <Button
              type="submit"
              disabled={saving || isSubmitting || (type === 'hass' ? !hassUrl.trim() || !hassToken.trim() : !name.trim())}
            >
              {saving || isSubmitting ? t('devices:connectionDialog.saving') : t('devices:connectionDialog.save')}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  )
}

// View Devices Dialog
function DevicesDialog({ open, onClose, devices, connectionName }: {
  open: boolean
  onClose: () => void
  devices: any[]
  connectionName: string
}) {
  const { t } = useTranslation(['devices'])

  return (
    <Dialog open={open} onOpenChange={onClose}>
      <DialogContent className="max-w-2xl">
        <DialogHeader>
          <DialogTitle>{t('devices:devicesDialog.title')}</DialogTitle>
          <DialogDescription>
            {t('devices:devicesDialog.description', { connection: connectionName })}
          </DialogDescription>
        </DialogHeader>
        {devices.length === 0 ? (
          <EmptyState
            title={t('devices:devicesDialog.noDevices')}
            description={t('devices:devicesDialog.noDevicesDesc')}
          />
        ) : (
          <div className="max-h-96 overflow-y-auto space-y-2">
            {devices.map((device) => (
              <div key={device.id} className="flex items-center justify-between p-3 border rounded-lg">
                <div className="flex items-center gap-3">
                  <Wifi className={cn(
                    "h-4 w-4",
                    device.status === "online" ? "text-green-500" : "text-muted-foreground"
                  )} />
                  <div>
                    <div className="font-medium">{device.name || device.id}</div>
                    <div className="text-sm text-muted-foreground">{device.device_type}</div>
                  </div>
                </div>
                <Badge variant={device.status === "online" ? "default" : "secondary"}>
                  {device.status === "online" ? t('devices:devicesDialog.online') : t('devices:devicesDialog.offline')}
                </Badge>
              </div>
            ))}
          </div>
        )}
      </DialogContent>
    </Dialog>
  )
}
