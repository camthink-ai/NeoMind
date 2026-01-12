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

// Connection type definitions
const CONNECTION_TYPES: Record<string, {
  id: string
  name: string
  icon: React.ReactNode
  iconBg: string
  description: string
  canAddMultiple: boolean
  builtin: boolean
}> = {
  builtin_mqtt: {
    id: 'builtin_mqtt',
    name: '内置 MQTT Broker',
    icon: <Server className="h-6 w-6" />,
    iconBg: 'bg-green-100 text-green-700 dark:bg-green-900/20 dark:text-green-400',
    description: '系统内置的 MQTT 消息服务器',
    canAddMultiple: false,
    builtin: true,
  },
  external_mqtt: {
    id: 'external_mqtt',
    name: '外部 MQTT Broker',
    icon: <Network className="h-6 w-6" />,
    iconBg: 'bg-blue-100 text-blue-700 dark:bg-blue-900/20 dark:text-blue-400',
    description: '连接到外部 MQTT 服务器',
    canAddMultiple: true,
    builtin: false,
  },
  hass: {
    id: 'hass',
    name: 'Home Assistant',
    icon: <Home className="h-6 w-6" />,
    iconBg: 'bg-purple-100 text-purple-700 dark:bg-purple-900/20 dark:text-purple-400',
    description: 'Home Assistant REST API 集成',
    canAddMultiple: false,
    builtin: false,
  },
  modbus: {
    id: 'modbus',
    name: 'Modbus TCP',
    icon: <Wifi className="h-6 w-6" />,
    iconBg: 'bg-orange-100 text-orange-700 dark:bg-orange-900/20 dark:text-orange-400',
    description: 'Modbus TCP 工业设备连接',
    canAddMultiple: true,
    builtin: false,
  },
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
    if (type === 'builtin_mqtt') {
      return devices.filter((d: any) =>
        !d.plugin_id || d.plugin_id === 'internal-mqtt' || d.plugin_id === 'builtin'
      ).length
    } else if (type === 'external_mqtt' && id) {
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
    if (type === 'builtin_mqtt') {
      return mqttStatus?.connected || false
    } else if (type === 'external_mqtt') {
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

    if (type === 'builtin_mqtt') {
      filteredDevices = devices.filter((d: any) =>
        !d.plugin_id || d.plugin_id === 'internal-mqtt' || d.plugin_id === 'builtin'
      )
      setConnectionName('内置 MQTT Broker')
    } else if (type === 'external_mqtt' && id) {
      filteredDevices = devices.filter((d: any) => d.plugin_id === id)
      const broker = externalBrokers.find((b) => b.id === id)
      setConnectionName(broker?.name || '外部 MQTT')
    } else if (type === 'hass') {
      filteredDevices = devices.filter((d: any) => d.plugin_id === 'hass-discovery')
      setConnectionName('Home Assistant')
    } else if (type === 'modbus' && id) {
      filteredDevices = devices.filter((d: any) => d.plugin_id === id)
      const adapter = modbusAdapters.find((a) => a.id === id)
      setConnectionName(adapter?.name || 'Modbus')
    }

    setConnectionDevices(filteredDevices)
    setDevicesDialogOpen(true)
  }

  const handleDelete = async (type: string, id: string) => {
    if (type === 'external_mqtt') {
      if (!confirm(`确定要删除这个 MQTT Broker 连接吗？`)) return
      try {
        await api.deleteBroker(id)
        await loadData()
        toast({ title: '已删除', description: 'MQTT Broker 已删除' })
      } catch (error) {
        toast({ title: '删除失败', description: (error as Error).message, variant: 'destructive' })
      }
    } else if (type === 'modbus') {
      if (!confirm(`确定要删除这个 Modbus 连接吗？`)) return
      try {
        await api.unregisterPlugin(id)
        await loadData()
        toast({ title: '已删除', description: 'Modbus 连接已删除' })
      } catch (error) {
        toast({ title: '删除失败', description: (error as Error).message, variant: 'destructive' })
      }
    } else if (type === 'hass') {
      toast({
        title: '提示',
        description: '请在设置页面禁用 Home Assistant 集成',
      })
    }
  }

  const handleTest = async (type: string, id?: string) => {
    if (type === 'external_mqtt' && id) {
      try {
        const result = await api.testBroker(id)
        if (result.success) {
          toast({ title: '连接成功', description: result.message })
        } else {
          toast({ title: '连接失败', description: result.message, variant: 'destructive' })
        }
        await loadData()
      } catch (error) {
        toast({ title: '测试失败', description: (error as Error).message, variant: 'destructive' })
      }
    } else {
      toast({ title: '提示', description: '此连接类型不支持测试连接' })
    }
  }

  const handleToggle = async (type: string, id?: string) => {
    if (type === 'external_mqtt' && id) {
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

        toast({
          title: !broker.enabled ? '已启用' : '已禁用',
          description: `MQTT Broker 已${!broker.enabled ? '启用' : '禁用'}`
        })
        await loadData()
      } catch (error) {
        toast({ title: '操作失败', description: (error as Error).message, variant: 'destructive' })
      }
    } else {
      toast({ title: '提示', description: '此连接类型不支持切换状态' })
    }
  }

  if (loading) {
    return <LoadingState text="加载中..." />
  }

  // ========== LIST VIEW ==========
  if (view === 'list') {
    return (
      <>
        {/* Header */}
        <div className="flex items-center justify-between mb-4">
          <div>
            <h2 className="text-2xl font-bold tracking-tight">{t('devices:deviceAdapters')}</h2>
            <p className="text-muted-foreground text-sm">
              管理设备连接方式，接入各种协议的设备
            </p>
          </div>
        </div>

        {/* Connection Type Cards */}
        <div className="grid gap-4 md:grid-cols-2 lg:grid-cols-4">
          {Object.values(CONNECTION_TYPES).map((type) => {
            const isActive = getConnectionStatus(type.id)
            const deviceCount = getDeviceCount(type.id)

            return (
              <Card
                key={type.id}
                className={cn(
                  "cursor-pointer transition-all duration-200 hover:shadow-md",
                  isActive && "border-green-500 border-2"
                )}
                onClick={() => {
                  setSelectedType(type.id)
                  setView('detail')
                }}
              >
                <CardHeader className="pb-3">
                  <div className={cn("flex items-center justify-center w-12 h-12 rounded-lg", type.iconBg)}>
                    {type.icon}
                  </div>
                  <CardTitle className="text-base mt-3">{type.name}</CardTitle>
                  <CardDescription className="mt-1 text-xs">
                    {type.description}
                  </CardDescription>
                </CardHeader>
                <CardContent className="text-sm">
                  <div className="flex justify-between items-center">
                    <span className="text-muted-foreground">状态:</span>
                    <span className={isActive ? "text-green-600 font-medium" : "text-muted-foreground font-medium"}>
                      {isActive ? "运行中" : "未配置"}
                    </span>
                  </div>
                  <div className="flex justify-between items-center mt-2">
                    <span className="text-muted-foreground">设备:</span>
                    <span className="font-medium">{deviceCount} 个</span>
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
    const typeInfo = CONNECTION_TYPES[selectedType]

    return (
      <>
        {/* Header with back button */}
        <div className="flex items-center gap-4 mb-4">
          <Button variant="ghost" size="sm" onClick={() => setView('list')} className="gap-1">
            <ArrowLeft className="h-4 w-4" />
            返回
          </Button>
          <div className="flex items-center gap-3">
            <div className={cn("flex items-center justify-center w-10 h-10 rounded-lg", typeInfo.iconBg)}>
              {typeInfo.icon}
            </div>
            <div>
              <h2 className="text-2xl font-bold">{typeInfo.name}</h2>
              <p className="text-sm text-muted-foreground">{typeInfo.description}</p>
            </div>
          </div>
          {typeInfo.canAddMultiple && (
            <div className="ml-auto">
              <Button onClick={() => {
                setEditingItem(null)
                setConfigDialogOpen(true)
              }}>
                <Plus className="mr-2 h-4 w-4" />
                添加连接
              </Button>
            </div>
          )}
        </div>

        {/* Builtin MQTT Detail */}
        {selectedType === 'builtin_mqtt' && (
          <BuiltinMqttCard
            mqttStatus={mqttStatus}
            deviceCount={getDeviceCount('builtin_mqtt')}
            onViewDevices={() => handleViewDevices('builtin_mqtt')}
            onRefresh={loadData}
          />
        )}

        {/* External MQTT Detail */}
        {selectedType === 'external_mqtt' && (
          <div className="space-y-4">
            {externalBrokers.length === 0 ? (
              <Card className="border-dashed">
                <CardContent className="flex flex-col items-center justify-center py-12">
                  <div className={cn("flex items-center justify-center w-16 h-16 rounded-lg mb-4", typeInfo.iconBg)}>
                    {typeInfo.icon}
                  </div>
                  <h3 className="text-lg font-semibold mb-1">暂无外部 MQTT 连接</h3>
                  <p className="text-sm text-muted-foreground mb-4">
                    添加外部 MQTT Broker 以扩展设备接入能力
                  </p>
                  <Button onClick={() => {
                    setEditingItem(null)
                    setConfigDialogOpen(true)
                  }}>
                    <Plus className="mr-2 h-4 w-4" />
                    添加 MQTT Broker
                  </Button>
                </CardContent>
              </Card>
            ) : (
              externalBrokers.map((broker) => (
                <ExternalBrokerCard
                  key={broker.id}
                  broker={broker}
                  deviceCount={getDeviceCount('external_mqtt', broker.id)}
                  onEdit={() => {
                    setEditingItem(broker)
                    setConfigDialogOpen(true)
                  }}
                  onDelete={() => handleDelete('external_mqtt', broker.id)}
                  onTest={() => handleTest('external_mqtt', broker.id)}
                  onToggle={() => handleToggle('external_mqtt', broker.id)}
                  onViewDevices={() => handleViewDevices('external_mqtt', broker.id)}
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
                  <div className={cn("flex items-center justify-center w-16 h-16 rounded-lg mb-4", typeInfo.iconBg)}>
                    {typeInfo.icon}
                  </div>
                  <h3 className="text-lg font-semibold mb-1">暂无 Modbus 连接</h3>
                  <p className="text-sm text-muted-foreground mb-4">
                    添加 Modbus TCP 设备连接
                  </p>
                  <Button onClick={() => {
                    setEditingItem(null)
                    setConfigDialogOpen(true)
                  }}>
                    <Plus className="mr-2 h-4 w-4" />
                    添加 Modbus 连接
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
  const connected = mqttStatus?.connected || false

  return (
    <Card className={cn("transition-all duration-200", connected && "border-green-500")}>
      <CardHeader className="pb-3">
        <div className="flex items-start justify-between">
          <div className="flex items-center gap-3">
            <div className="p-2 rounded-lg bg-green-100 text-green-700 dark:bg-green-900/20 dark:text-green-400">
              <Server className="h-5 w-5" />
            </div>
            <div>
              <CardTitle className="text-base">内置 MQTT Broker</CardTitle>
              <CardDescription className="text-xs">
                {mqttStatus?.server_ip}:{mqttStatus?.listen_port || 1883}
              </CardDescription>
            </div>
          </div>
          <Badge variant={connected ? "default" : "secondary"}>
            {connected ? "运行中" : "已停止"}
          </Badge>
        </div>
      </CardHeader>
      <CardContent className="pb-3">
        <div className="space-y-2 text-sm">
          <div className="flex items-center justify-between">
            <span className="text-muted-foreground">状态:</span>
            <span className={connected ? "text-green-600 font-medium" : "text-muted-foreground font-medium"}>
              {connected ? "已连接" : "未连接"}
            </span>
          </div>
          <div className="flex items-center justify-between">
            <span className="text-muted-foreground">设备数:</span>
            <span className="font-medium">{deviceCount}</span>
          </div>
        </div>
      </CardContent>
      <CardFooter className="pt-3 border-t justify-between">
        <Button variant="outline" size="sm" onClick={onViewDevices}>
          <Wifi className="mr-2 h-4 w-4" />
          查看设备
        </Button>
        <Button variant="ghost" size="sm" onClick={onRefresh}>
          刷新状态
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
                {connected ? "已连接" : "未连接"}
              </Badge>
              <Badge variant={enabled ? "outline" : "secondary"} className="text-xs">
                {enabled ? "已启用" : "已禁用"}
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
                {enabled ? "禁用" : "启用"}
              </DropdownMenuItem>
              <DropdownMenuItem onClick={onEdit}>
                <Edit className="mr-2 h-4 w-4" />
                编辑
              </DropdownMenuItem>
              <DropdownMenuItem onClick={onTest}>
                <TestTube className="mr-2 h-4 w-4" />
                测试连接
              </DropdownMenuItem>
              <DropdownMenuSeparator />
              <DropdownMenuItem onClick={onDelete} className="text-destructive focus:text-destructive">
                <Trash2 className="mr-2 h-4 w-4" />
                删除
              </DropdownMenuItem>
            </DropdownMenuContent>
          </DropdownMenu>
        </div>
      </CardHeader>
      <CardContent className="pb-3">
        <div className="space-y-2 text-sm">
          <div className="flex items-center justify-between">
            <span className="text-muted-foreground">设备数:</span>
            <span className="font-medium">{deviceCount}</span>
          </div>
          {broker.tls && (
            <div className="flex items-center gap-2">
              <CheckCircle2 className="h-3 w-3 text-green-600" />
              <span className="text-xs text-muted-foreground">启用 TLS</span>
            </div>
          )}
          {broker.username && (
            <div className="flex items-center justify-between">
              <span className="text-muted-foreground">认证:</span>
              <span className="text-xs">已配置</span>
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
          查看设备
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
              <CardTitle className="text-base">Home Assistant 集成</CardTitle>
              <CardDescription className="text-xs">
                {hassStatus?.hass_integration?.url || '未配置'}
              </CardDescription>
            </div>
          </div>
          <Badge variant={connected ? "default" : "secondary"}>
            {connected ? "已连接" : configured ? "未连接" : "未配置"}
          </Badge>
        </div>
      </CardHeader>
      <CardContent className="pb-3">
        <div className="space-y-2 text-sm">
          <div className="flex items-center justify-between">
            <span className="text-muted-foreground">连接状态:</span>
            <span className={connected ? "text-green-600 font-medium" : "text-muted-foreground font-medium"}>
              {connected ? "已连接" : "未连接"}
            </span>
          </div>
          <div className="flex items-center justify-between">
            <span className="text-muted-foreground">设备数:</span>
            <span className="font-medium">{deviceCount}</span>
          </div>
          <p className="text-xs text-muted-foreground mt-2">
            通过 REST API 与 Home Assistant 集成，用于查询和控制 HASS 设备
          </p>
          <p className="text-xs text-muted-foreground">
            HASS MQTT 设备发现功能已移至设备模块
          </p>
        </div>
      </CardContent>
      <CardFooter className="pt-3 border-t flex flex-wrap gap-2">
        <Button variant="outline" size="sm" onClick={onViewDevices}>
          <Wifi className="mr-2 h-4 w-4" />
          查看设备
        </Button>
        <div className="flex gap-2 ml-auto">
          <Button variant="ghost" size="sm" onClick={onRefresh}>
            <RefreshCw className="mr-2 h-4 w-4" />
            刷新
          </Button>
          <Button variant="ghost" size="sm" onClick={onConfigure}>
            <Settings className="mr-2 h-4 w-4" />
            配置
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
  const running = adapter.running || false

  return (
    <Card className={cn("transition-all duration-200", running && "border-green-500")}>
      <CardHeader className="pb-3">
        <div className="flex items-start justify-between">
          <div className="flex-1">
            <div className="flex items-center gap-2 mb-1">
              <CardTitle className="text-base">{adapter.name}</CardTitle>
              <Badge variant={running ? "default" : "secondary"} className="text-xs">
                {running ? "运行中" : "已停止"}
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
                编辑
              </DropdownMenuItem>
              <DropdownMenuSeparator />
              <DropdownMenuItem onClick={onDelete} className="text-destructive focus:text-destructive">
                <Trash2 className="mr-2 h-4 w-4" />
                删除
              </DropdownMenuItem>
            </DropdownMenuContent>
          </DropdownMenu>
        </div>
      </CardHeader>
      <CardContent className="pb-3">
        <div className="space-y-2 text-sm">
          <div className="flex items-center justify-between">
            <span className="text-muted-foreground">从站 ID:</span>
            <span className="font-medium">{adapter.config?.slave_id}</span>
          </div>
          <div className="flex items-center justify-between">
            <span className="text-muted-foreground">设备数:</span>
            <span className="font-medium">{deviceCount}</span>
          </div>
        </div>
      </CardContent>
      <CardFooter className="pt-3 border-t">
        <Button variant="outline" size="sm" onClick={onViewDevices}>
          <Wifi className="mr-2 h-4 w-4" />
          查看设备
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
      if (type === 'external_mqtt') {
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

    try {
      if (type === 'external_mqtt') {
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
    }
  }

  if (!open) return null

  const typeInfo = CONNECTION_TYPES[type]

  return (
    <Dialog open={open} onOpenChange={onClose}>
      <DialogContent className="max-w-md">
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2">
            <div className={cn("p-2 rounded-lg", typeInfo?.iconBg)}>
              {typeInfo?.icon}
            </div>
            {editing ? "编辑" : "添加"} {typeInfo?.name}
          </DialogTitle>
          <DialogDescription>
            {editing ? "修改" : "配置"} {typeInfo?.name} 连接信息
          </DialogDescription>
        </DialogHeader>

        <form onSubmit={handleSubmit} className="space-y-4">
          <div>
            <Label htmlFor="name">显示名称 *</Label>
            <Input
              id="name"
              value={name}
              onChange={(e) => setName(e.target.value)}
              placeholder={type === 'external_mqtt' ? '我的 MQTT Broker' : 'Modbus 设备'}
              required
            />
          </div>

          {type === 'external_mqtt' && (
            <>
              <div>
                <Label htmlFor="broker">Broker 地址 *</Label>
                <Input
                  id="broker"
                  value={broker}
                  onChange={(e) => setBroker(e.target.value)}
                  placeholder="broker.example.com"
                  required
                />
              </div>
              <div>
                <Label htmlFor="port">端口</Label>
                <Input
                  id="port"
                  type="number"
                  value={port}
                  onChange={(e) => setPort(parseInt(e.target.value) || 1883)}
                />
              </div>
              <div>
                <Label htmlFor="username">用户名（可选）</Label>
                <Input
                  id="username"
                  value={username}
                  onChange={(e) => setUsername(e.target.value)}
                  placeholder="username"
                />
              </div>
              <div>
                <Label htmlFor="password">密码（可选）</Label>
                <Input
                  id="password"
                  type="password"
                  value={password}
                  onChange={(e) => setPassword(e.target.value)}
                  placeholder={editing ? '留空保持不变' : '••••••••'}
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
                <Label htmlFor="tls" className="cursor-pointer">启用 TLS/MQTTS</Label>
              </div>

              {/* TLS Certificate Configuration */}
              {tls && (
                <div className="space-y-3 p-3 border rounded-md bg-muted/30">
                  <div className="text-xs font-medium text-blue-600 dark:text-blue-400">
                    🔒 TLS 证书配置
                  </div>
                  <div>
                    <Label htmlFor="caCert" className="text-xs">CA 证书 (PEM) - 可选</Label>
                    <textarea
                      id="caCert"
                      value={caCert}
                      onChange={(e) => setCaCert(e.target.value)}
                      placeholder="-----BEGIN CERTIFICATE-----"
                      className="w-full min-h-[60px] px-2 py-1 text-xs font-mono border rounded-md resize-none bg-background"
                    />
                  </div>
                  <div>
                    <Label htmlFor="clientCert" className="text-xs">客户端证书 (PEM) - 可选</Label>
                    <textarea
                      id="clientCert"
                      value={clientCert}
                      onChange={(e) => setClientCert(e.target.value)}
                      placeholder="-----BEGIN CERTIFICATE-----"
                      className="w-full min-h-[60px] px-2 py-1 text-xs font-mono border rounded-md resize-none bg-background"
                    />
                  </div>
                  <div>
                    <Label htmlFor="clientKey" className="text-xs">客户端私钥 (PEM) - 可选</Label>
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
                <Label htmlFor="modbusHost">主机地址 *</Label>
                <Input
                  id="modbusHost"
                  value={modbusHost}
                  onChange={(e) => setModbusHost(e.target.value)}
                  placeholder="192.168.1.100"
                  required
                />
              </div>
              <div>
                <Label htmlFor="modbusPort">端口</Label>
                <Input
                  id="modbusPort"
                  type="number"
                  value={modbusPort}
                  onChange={(e) => setModbusPort(parseInt(e.target.value) || 502)}
                />
              </div>
              <div>
                <Label htmlFor="slaveId">从站 ID</Label>
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
                <Label htmlFor="hassUrl">Home Assistant URL *</Label>
                <Input
                  id="hassUrl"
                  type="url"
                  value={hassUrl}
                  onChange={(e) => setHassUrl(e.target.value)}
                  placeholder="http://homeassistant.local:8123"
                  required
                />
                <p className="text-xs text-muted-foreground mt-1">
                  Home Assistant 的访问地址
                </p>
              </div>
              <div>
                <Label htmlFor="hassToken">访问令牌 (Long-Lived Access Token) *</Label>
                <Input
                  id="hassToken"
                  type="password"
                  value={hassToken}
                  onChange={(e) => setHassToken(e.target.value)}
                  placeholder="eyJ0eXAiOiAi..."
                  required
                />
                <p className="text-xs text-muted-foreground mt-1">
                  在 Home Assistant 用户设置中生成
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
                <Label htmlFor="hassVerifySsl" className="cursor-pointer">验证 SSL 证书</Label>
              </div>
              <div className="flex items-center gap-2">
                <input
                  type="checkbox"
                  id="hassAutoImport"
                  checked={hassAutoImport}
                  onChange={(e) => setHassAutoImport(e.target.checked)}
                  className="h-4 w-4"
                />
                <Label htmlFor="hassAutoImport" className="cursor-pointer">自动导入发现的设备</Label>
              </div>
            </>
          )}

          <DialogFooter>
            <Button type="button" variant="outline" onClick={onClose} disabled={saving}>
              取消
            </Button>
            <Button
              type="submit"
              disabled={saving || (type === 'hass' ? !hassUrl.trim() || !hassToken.trim() : !name.trim())}
            >
              {saving ? "保存中..." : "保存"}
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
  return (
    <Dialog open={open} onOpenChange={onClose}>
      <DialogContent className="max-w-2xl">
        <DialogHeader>
          <DialogTitle>连接的设备</DialogTitle>
          <DialogDescription>
            {connectionName} 管理的设备列表
          </DialogDescription>
        </DialogHeader>
        {devices.length === 0 ? (
          <EmptyState
            title="暂无设备"
            description="此连接当前没有管理的设备"
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
                  {device.status === "online" ? "在线" : "离线"}
                </Badge>
              </div>
            ))}
          </div>
        )}
      </DialogContent>
    </Dialog>
  )
}
