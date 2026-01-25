/**
 * DataSourceSelector Component
 *
 * Dialog for selecting data sources with device metrics/commands/basic info.
 * New design: Devices are containers that expand to show their data types.
 */

import { useState, useEffect, useRef, useMemo } from 'react'
import { Search, Server, Database, Check, Zap, ChevronRight, BarChart3, Bot, Info, Layers } from 'lucide-react'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import { Tabs, TabsList, TabsTrigger, TabsContent } from '@/components/ui/tabs'
import { useStore } from '@/store'
import type { DataSource, DataSourceOrList } from '@/types/dashboard'
import { normalizeDataSource } from '@/types/dashboard'
import type { MetricDefinition, CommandDefinition } from '@/types'
import { cn } from '@/lib/utils'

export interface DataSourceSelectorProps {
  open: boolean
  onClose: () => void
  onSelect: (dataSource: DataSourceOrList | DataSource | undefined) => void
  currentDataSource?: DataSourceOrList
  // Optional: filter which source types to show
  allowedTypes?: Array<'device-metric' | 'device-command' | 'device-info' | 'agent' | 'system' | 'device' | 'metric' | 'command'>
  // Optional: enable multiple data source selection
  multiple?: boolean
  // Optional: max number of data sources (only used when multiple is true)
  maxSources?: number
}

type CategoryType = 'device-metric' | 'device-command' | 'device-info' | 'agent' | 'system'
type SelectedItem = string // Format: "device-metric:deviceId:property" or "device-command:deviceId:command" etc.

// Device info property definitions
const DEVICE_INFO_PROPERTIES = [
  { id: 'name', name: '设备名称', description: '设备的显示名称' },
  { id: 'status', name: '状态', description: '当前状态文本' },
  { id: 'online', name: '在线状态', description: '是否在线' },
  { id: 'last_seen', name: '最后上线', description: '最后通信时间' },
  { id: 'device_type', name: '设备类型', description: '设备类型标识' },
  { id: 'plugin_name', name: '适配器', description: '连接的插件名称' },
  { id: 'adapter_id', name: '适配器ID', description: '适配器唯一标识' },
]

// Category configuration
const CATEGORIES = [
  { id: 'device-metric' as const, name: '指标', icon: Server, description: '设备的实时数据点' },
  { id: 'device-command' as const, name: '指令', icon: Zap, description: '控制设备的操作' },
  { id: 'device-info' as const, name: '基本信息', icon: Info, description: '设备的属性和状态' },
  { id: 'agent' as const, name: 'Agent', icon: Bot, description: 'AI Agent 数据' },
  { id: 'system' as const, name: '系统', icon: BarChart3, description: '系统统计数据' },
]

// Convert old allowedTypes format to new format
function normalizeAllowedTypes(
  allowedTypes?: Array<'device-metric' | 'device-command' | 'device-info' | 'agent' | 'system' | 'device' | 'metric' | 'command'>
): CategoryType[] {
  if (!allowedTypes) return ['device-metric', 'device-command', 'device-info', 'agent', 'system']

  const result: CategoryType[] = []

  // New format types
  if (allowedTypes.includes('device-metric')) result.push('device-metric')
  if (allowedTypes.includes('device-command')) result.push('device-command')
  if (allowedTypes.includes('device-info')) result.push('device-info')
  if (allowedTypes.includes('agent')) result.push('agent')
  if (allowedTypes.includes('system')) result.push('system')

  // Old format types - map to new format
  if (allowedTypes.includes('device') || allowedTypes.includes('metric')) {
    if (!result.includes('device-metric')) result.push('device-metric')
  }
  if (allowedTypes.includes('command')) {
    if (!result.includes('device-command')) result.push('device-command')
  }

  return result.length > 0 ? result : ['device-metric', 'device-command', 'device-info', 'agent', 'system']
}

// System metrics (non-device specific)
const SYSTEM_METRICS = [
  { id: 'device-online-count', name: '在线设备数', unit: '个', endpoint: '/stats/devices' },
  { id: 'device-offline-count', name: '离线设备数', unit: '个', endpoint: '/stats/devices' },
  { id: 'alert-critical-count', name: '严重告警', unit: '个', endpoint: '/stats/alerts' },
  { id: 'alert-warning-count', name: '警告告警', unit: '个', endpoint: '/stats/alerts' },
  { id: 'rule-trigger-count', name: '规则触发次数', unit: '次', endpoint: '/stats/rules' },
]

// Agent data options
const AGENT_METRICS = [
  { id: 'agent-status', name: 'Agent 状态', description: 'idle, running, paused' },
  { id: 'agent-executions', name: '执行次数', description: '总执行次数' },
  { id: 'agent-success-rate', name: '成功率', description: '执行成功率百分比' },
]

export function DataSourceSelector({
  open,
  onClose,
  onSelect,
  currentDataSource,
  allowedTypes,
  multiple = false,
  maxSources = 10,
}: DataSourceSelectorProps) {
  const { devices, deviceTypes, fetchDeviceTypes, fetchDevices } = useStore()
  const [searchQuery, setSearchQuery] = useState('')
  const [selectedCategory, setSelectedCategory] = useState<CategoryType>('device-metric')
  const [selectedItems, setSelectedItems] = useState<Set<SelectedItem>>(new Set())
  const [expandedDevices, setExpandedDevices] = useState<Set<string>>(new Set())

  // Track initialization
  const initializedRef = useRef(false)
  const prevOpenRef = useRef(false)

  // Fetch device types and devices when dialog opens
  useEffect(() => {
    if (open) {
      if (deviceTypes.length === 0) {
        fetchDeviceTypes()
      }
      if (devices.length === 0) {
        fetchDevices()
      }
    }
  }, [open, deviceTypes.length, devices.length, fetchDeviceTypes, fetchDevices])

  // Filter allowed categories
  const availableCategories = useMemo(() => {
    const allowed = normalizeAllowedTypes(allowedTypes)
    return CATEGORIES.filter(cat => allowed.includes(cat.id))
  }, [allowedTypes])

  // Set initial category based on allowed types
  useEffect(() => {
    if (availableCategories.length > 0 && !availableCategories.find(c => c.id === selectedCategory)) {
      setSelectedCategory(availableCategories[0].id)
    }
  }, [availableCategories, selectedCategory])

  // Normalize current data source to array
  const currentDataSources = normalizeDataSource(currentDataSource)

  // Initialize selection when dialog opens
  useEffect(() => {
    if (open && !prevOpenRef.current) {
      const newSelectedItems = new Set<SelectedItem>()

      for (const ds of currentDataSources) {
        if (ds.type === 'device' && ds.deviceId && ds.property) {
          newSelectedItems.add(`device-metric:${ds.deviceId}:${ds.property}`)
        } else if (ds.type === 'command' && ds.deviceId && ds.command) {
          newSelectedItems.add(`device-command:${ds.deviceId}:${ds.command}`)
        } else if (ds.type === 'device-info' && ds.deviceId && ds.infoProperty) {
          newSelectedItems.add(`device-info:${ds.deviceId}:${ds.infoProperty}`)
        } else if (ds.type === 'api' && ds.endpoint) {
          // System metrics
          if (ds.endpoint.includes('devices')) {
            newSelectedItems.add(`system:device-online-count`)
          } else if (ds.endpoint.includes('alerts')) {
            newSelectedItems.add(`system:alert-critical-count`)
          }
        } else if (ds.type === 'static') {
          // Map to system
          newSelectedItems.add(`system:device-online-count`)
        }
      }

      setSelectedItems(newSelectedItems)
      initializedRef.current = true
    }

    if (!open) {
      initializedRef.current = false
    }

    prevOpenRef.current = open
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open])

  // Build device metrics map
  const deviceMetricsMap = useMemo(() => {
    const map = new Map<string, MetricDefinition[]>()
    for (const device of devices) {
      const deviceType = deviceTypes.find(dt => dt.device_type === device.device_type)

      if (deviceType?.metrics && deviceType.metrics.length > 0) {
        map.set(device.id, deviceType.metrics)
      } else {
        // Fallback metrics for devices without type definition
        const fallbackMetrics: MetricDefinition[] = [
          { name: 'temperature', display_name: '温度', data_type: 'float', unit: '°C' },
          { name: 'humidity', display_name: '湿度', data_type: 'float', unit: '%' },
          { name: 'value', display_name: '数值', data_type: 'float', unit: '' },
          { name: 'state', display_name: '状态', data_type: 'string', unit: '' },
          { name: 'status', display_name: '状态', data_type: 'string', unit: '' },
          { name: 'online', display_name: '在线状态', data_type: 'boolean', unit: '' },
        ]
        // Try to use current_values from device to infer available metrics
        if (device.current_values && typeof device.current_values === 'object') {
          const dynamicMetrics: MetricDefinition[] = Object.keys(device.current_values).map(key => ({
            name: key,
            display_name: key,
            data_type: 'float' as const,
            unit: '',
          }))
          map.set(device.id, dynamicMetrics)
        } else {
          map.set(device.id, fallbackMetrics)
        }
      }
    }
    return map
  }, [devices, deviceTypes])

  // Build device commands map
  const deviceCommandsMap = useMemo(() => {
    const map = new Map<string, CommandDefinition[]>()
    for (const device of devices) {
      const deviceType = deviceTypes.find(dt => dt.device_type === device.device_type)

      if (deviceType?.commands && deviceType.commands.length > 0) {
        map.set(device.id, deviceType.commands)
      } else {
        // Fallback commands for devices without type definition
        const fallbackCommands: CommandDefinition[] = [
          { name: 'setValue', display_name: '设置值', payload_template: '${value}', parameters: [] },
          { name: 'toggle', display_name: '切换', payload_template: '', parameters: [] },
          { name: 'on', display_name: '开启', payload_template: '{"state":"on"}', parameters: [] },
          { name: 'off', display_name: '关闭', payload_template: '{"state":"off"}', parameters: [] },
        ]
        map.set(device.id, fallbackCommands)
      }
    }
    return map
  }, [devices, deviceTypes])

  // Toggle device expansion
  const toggleDeviceExpansion = (deviceId: string) => {
    const newExpanded = new Set(expandedDevices)
    if (newExpanded.has(deviceId)) {
      newExpanded.delete(deviceId)
    } else {
      newExpanded.add(deviceId)
    }
    setExpandedDevices(newExpanded)
  }

  // Handle item selection
  const handleItemClick = (itemId: SelectedItem) => {
    if (multiple) {
      const newSelectedItems = new Set(selectedItems)
      if (newSelectedItems.has(itemId)) {
        newSelectedItems.delete(itemId)
      } else {
        if (newSelectedItems.size >= maxSources) {
          return
        }
        newSelectedItems.add(itemId)
      }
      setSelectedItems(newSelectedItems)
    } else {
      setSelectedItems(new Set([itemId]))
    }
  }

  // Convert selected items to DataSource(s)
  const handleSelect = () => {
    if (selectedItems.size === 0) return

    const createDataSource = (itemId: SelectedItem): DataSource => {
      const [category, ...rest] = itemId.split(':')

      if (category === 'device-metric') {
        const [deviceId, property] = rest
        return {
          type: 'device',
          deviceId,
          property,
          refresh: 5,
        }
      } else if (category === 'device-command') {
        const [deviceId, command] = rest
        return {
          type: 'command',
          deviceId,
          command,
          property: 'state',
          valueMapping: { on: true, off: false },
        }
      } else if (category === 'device-info') {
        const [deviceId, infoProperty] = rest
        return {
          type: 'device-info',
          deviceId,
          infoProperty: infoProperty as any,
          refresh: 10,
        }
      } else if (category === 'system') {
        const metricId = rest[0]
        const metric = SYSTEM_METRICS.find(m => m.id === metricId)
        return {
          type: 'api',
          endpoint: metric?.endpoint || '/stats/devices',
          refresh: 10,
        }
      } else if (category === 'agent') {
        const metricId = rest[0]
        return {
          type: 'api',
          endpoint: '/agents',
          params: { metric: metricId },
          refresh: 10,
        }
      }

      // Fallback
      return {
        type: 'static',
        staticValue: 0,
      }
    }

    const dataSources = Array.from(selectedItems).map(createDataSource)

    if (multiple || dataSources.length > 1) {
      onSelect(dataSources)
    } else {
      onSelect(dataSources[0])
    }

    onClose()
  }

  const isSelected = (id: string) => selectedItems.has(id)
  const totalSelected = selectedItems.size
  const canSelectMore = totalSelected < maxSources

  // Filter by search query
  const filterMatches = (text: string) => {
    if (!searchQuery) return true
    const query = searchQuery.toLowerCase()
    return text.toLowerCase().includes(query)
  }

  // Get count for each category
  const getCategoryCount = (category: CategoryType) => {
    return Array.from(selectedItems).filter(id => id.startsWith(`${category}:`)).length
  }

  return (
    <Dialog open={open} onOpenChange={onClose}>
      <DialogContent className="sm:max-w-[600px] md:max-w-[700px] lg:max-w-[800px] p-0 gap-0 max-h-[90vh] overflow-hidden flex flex-col [&>[data-radix-dialog-close]]:right-6 [&>[data-radix-dialog-close]]:top-5">
        {/* Header */}
        <DialogHeader className="px-6 py-4 border-b shrink-0">
          <div className="flex items-center justify-between">
            <div className="flex items-center gap-3">
              <div className="w-9 h-9 rounded-lg bg-primary/10 flex items-center justify-center">
                <Database className="w-4.5 h-4.5 text-primary" />
              </div>
              <div>
                <DialogTitle className="text-base font-semibold p-0 h-auto">
                  选择数据源
                </DialogTitle>
                <p className="text-xs text-muted-foreground mt-0.5">
                  {multiple ? (
                    <>已选择 {totalSelected} / {maxSources} 项</>
                  ) : (
                    '选择设备指标、指令或基本信息'
                  )}
                </p>
              </div>
            </div>
            {multiple && totalSelected > 0 && (
              <div className="flex items-center gap-1.5 px-2.5 py-1 rounded-full bg-primary/10 text-primary text-sm font-medium">
                <Layers className="h-3.5 w-3.5" />
                {totalSelected}
              </div>
            )}
          </div>
        </DialogHeader>

        {/* Content */}
        <div className="flex-1 overflow-hidden flex flex-col">
          {/* Search */}
          <div className="px-6 py-3 border-b">
            <div className="relative">
              <Search className="absolute left-3 top-1/2 -translate-y-1/2 h-4 w-4 text-muted-foreground" />
              <Input
                value={searchQuery}
                onChange={(e) => setSearchQuery(e.target.value)}
                placeholder="搜索设备、指标或指令..."
                className="pl-9 h-9"
              />
            </div>
          </div>

          {/* Category Tabs */}
          <Tabs value={selectedCategory} onValueChange={(v) => setSelectedCategory(v as CategoryType)} className="flex-1 flex flex-col">
            <div className="px-6 pt-3">
              <TabsList className="grid w-full grid-cols-5 h-9 bg-muted/50">
                {availableCategories.map(cat => {
                  const Icon = cat.icon
                  const count = getCategoryCount(cat.id)
                  return (
                    <TabsTrigger
                      key={cat.id}
                      value={cat.id}
                      className="gap-1.5 data-[state=active]:bg-background text-xs relative"
                    >
                      <Icon className="h-3.5 w-3.5" />
                      {cat.name}
                      {count > 0 && (
                        <span className="absolute -top-1 -right-1 h-4 w-4 rounded-full bg-primary text-primary-foreground text-[10px] flex items-center justify-center">
                          {count}
                        </span>
                      )}
                    </TabsTrigger>
                  )
                })}
              </TabsList>
            </div>

            {/* Device Metrics Content */}
            <TabsContent value="device-metric" className="flex-1 overflow-y-auto px-6 py-4 mt-0">
              {devices.length > 0 ? (
                <div className="space-y-3">
                  {devices.map(device => {
                    const metrics = deviceMetricsMap.get(device.id) || []
                    const isExpanded = expandedDevices.has(device.id)
                    const hasMatchingMetric = metrics.some(m =>
                      filterMatches(m.display_name) || filterMatches(m.name) || filterMatches(device.name)
                    )

                    if (!hasMatchingMetric && searchQuery) return null

                    return (
                      <div key={device.id} className="border rounded-lg overflow-hidden">
                        {/* Device header */}
                        <button
                          onClick={() => toggleDeviceExpansion(device.id)}
                          className="w-full flex items-center justify-between p-3 bg-muted/30 hover:bg-muted/50 transition-colors"
                        >
                          <div className="flex items-center gap-3">
                            <div className={cn(
                              "w-2 h-2 rounded-full",
                              device.online ? 'bg-green-500' : 'bg-muted-foreground'
                            )} />
                            <span className="text-sm font-medium">{device.name}</span>
                            <span className="text-xs text-muted-foreground">
                              {metrics.length} 个指标
                            </span>
                          </div>
                          <ChevronRight className={cn(
                            "h-4 w-4 text-muted-foreground transition-transform",
                            isExpanded && 'rotate-90'
                          )} />
                        </button>

                        {/* Expanded metrics */}
                        {isExpanded && (
                          <div className="border-t divide-y max-h-60 overflow-y-auto">
                            {metrics.map(metric => {
                              const itemId = `device-metric:${device.id}:${metric.name}`
                              const selected = isSelected(itemId)
                              const disabled = multiple && !selected && !canSelectMore

                              if (!filterMatches(metric.display_name) && !filterMatches(metric.name)) return null

                              return (
                                <button
                                  key={metric.name}
                                  onClick={() => !disabled && handleItemClick(itemId)}
                                  disabled={disabled}
                                  className={cn(
                                    "w-full flex items-center justify-between px-3 py-2.5",
                                    "hover:bg-accent/50 transition-colors text-left",
                                    selected && 'bg-primary/5',
                                    disabled && 'opacity-50 cursor-not-allowed'
                                  )}
                                >
                                  <div className="flex items-center gap-2 flex-1">
                                    {multiple && (
                                      <div className={cn(
                                        "w-4 h-4 rounded border flex items-center justify-center flex-shrink-0",
                                        selected ? 'bg-primary border-primary' : 'border-border'
                                      )}>
                                        {selected && <Check className="h-3 w-3 text-white" />}
                                      </div>
                                    )}
                                    <div className="flex-1">
                                      <p className="text-sm font-medium">{metric.display_name}</p>
                                      <p className="text-xs text-muted-foreground">
                                        {metric.name} {metric.unit && `· ${metric.unit}`}
                                      </p>
                                    </div>
                                  </div>
                                  {!multiple && selected && (
                                    <Check className="h-4 w-4 text-primary flex-shrink-0" />
                                  )}
                                </button>
                              )
                            })}
                          </div>
                        )}
                      </div>
                    )
                  })}
                </div>
              ) : (
                <div className="text-center py-10 text-muted-foreground">
                  <Server className="h-10 w-10 mx-auto mb-3 opacity-40" />
                  <p className="text-sm">暂无设备</p>
                </div>
              )}
            </TabsContent>

            {/* Device Commands Content */}
            <TabsContent value="device-command" className="flex-1 overflow-y-auto px-6 py-4 mt-0">
              {devices.length > 0 ? (
                <div className="space-y-3">
                  {devices.map(device => {
                    const commands = deviceCommandsMap.get(device.id) || []
                    const isExpanded = expandedDevices.has(device.id)
                    const hasMatchingCommand = commands.some(c =>
                      filterMatches(c.display_name) || filterMatches(c.name) || filterMatches(device.name)
                    )

                    if (!hasMatchingCommand && searchQuery) return null

                    return (
                      <div key={device.id} className="border rounded-lg overflow-hidden">
                        <button
                          onClick={() => toggleDeviceExpansion(device.id)}
                          className="w-full flex items-center justify-between p-3 bg-muted/30 hover:bg-muted/50 transition-colors"
                        >
                          <div className="flex items-center gap-3">
                            <div className={cn(
                              "w-2 h-2 rounded-full",
                              device.online ? 'bg-green-500' : 'bg-muted-foreground'
                            )} />
                            <span className="text-sm font-medium">{device.name}</span>
                            <span className="text-xs text-muted-foreground">
                              {commands.length} 个指令
                            </span>
                          </div>
                          <ChevronRight className={cn(
                            "h-4 w-4 text-muted-foreground transition-transform",
                            isExpanded && 'rotate-90'
                          )} />
                        </button>

                        {isExpanded && (
                          <div className="border-t divide-y max-h-60 overflow-y-auto">
                            {commands.map(command => {
                              const itemId = `device-command:${device.id}:${command.name}`
                              const selected = isSelected(itemId)
                              const disabled = multiple && !selected && !canSelectMore

                              if (!filterMatches(command.display_name) && !filterMatches(command.name)) return null

                              return (
                                <button
                                  key={command.name}
                                  onClick={() => !disabled && handleItemClick(itemId)}
                                  disabled={disabled}
                                  className={cn(
                                    "w-full flex items-center justify-between px-3 py-2.5",
                                    "hover:bg-accent/50 transition-colors text-left",
                                    selected && 'bg-primary/5',
                                    disabled && 'opacity-50 cursor-not-allowed'
                                  )}
                                >
                                  <div className="flex items-center gap-2 flex-1">
                                    {multiple && (
                                      <div className={cn(
                                        "w-4 h-4 rounded border flex items-center justify-center flex-shrink-0",
                                        selected ? 'bg-primary border-primary' : 'border-border'
                                      )}>
                                        {selected && <Check className="h-3 w-3 text-white" />}
                                      </div>
                                    )}
                                    <div className="flex-1">
                                      <p className="text-sm font-medium">{command.display_name}</p>
                                      <p className="text-xs text-muted-foreground">{command.name}</p>
                                    </div>
                                  </div>
                                  {!multiple && selected && (
                                    <Check className="h-4 w-4 text-primary flex-shrink-0" />
                                  )}
                                </button>
                              )
                            })}
                          </div>
                        )}
                      </div>
                    )
                  })}
                </div>
              ) : (
                <div className="text-center py-10 text-muted-foreground">
                  <Zap className="h-10 w-10 mx-auto mb-3 opacity-40" />
                  <p className="text-sm">暂无设备</p>
                </div>
              )}
            </TabsContent>

            {/* Device Info Content */}
            <TabsContent value="device-info" className="flex-1 overflow-y-auto px-6 py-4 mt-0">
              {devices.length > 0 ? (
                <div className="space-y-3">
                  {devices.map(device => {
                    const isExpanded = expandedDevices.has(device.id)
                    const hasMatchingInfo = DEVICE_INFO_PROPERTIES.some(p =>
                      filterMatches(p.name) || filterMatches(p.description) || filterMatches(device.name)
                    )

                    if (!hasMatchingInfo && searchQuery) return null

                    return (
                      <div key={device.id} className="border rounded-lg overflow-hidden">
                        <button
                          onClick={() => toggleDeviceExpansion(device.id)}
                          className="w-full flex items-center justify-between p-3 bg-muted/30 hover:bg-muted/50 transition-colors"
                        >
                          <div className="flex items-center gap-3">
                            <div className={cn(
                              "w-2 h-2 rounded-full",
                              device.online ? 'bg-green-500' : 'bg-muted-foreground'
                            )} />
                            <span className="text-sm font-medium">{device.name}</span>
                            <span className="text-xs text-muted-foreground">
                              基本信息
                            </span>
                          </div>
                          <ChevronRight className={cn(
                            "h-4 w-4 text-muted-foreground transition-transform",
                            isExpanded && 'rotate-90'
                          )} />
                        </button>

                        {isExpanded && (
                          <div className="border-t divide-y max-h-60 overflow-y-auto">
                            {DEVICE_INFO_PROPERTIES.map(infoProp => {
                              const itemId = `device-info:${device.id}:${infoProp.id}`
                              const selected = isSelected(itemId)
                              const disabled = multiple && !selected && !canSelectMore

                              if (!filterMatches(infoProp.name) && !filterMatches(infoProp.description)) return null

                              // Show current value preview for some properties
                              const getPreviewValue = () => {
                                switch (infoProp.id) {
                                  case 'name': return device.name
                                  case 'status': return device.status
                                  case 'online': return device.online ? '在线' : '离线'
                                  case 'device_type': return device.device_type
                                  case 'plugin_name': return device.plugin_name || '-'
                                  case 'adapter_id': return device.adapter_id || '-'
                                  default: return '-'
                                }
                              }

                              return (
                                <button
                                  key={infoProp.id}
                                  onClick={() => !disabled && handleItemClick(itemId)}
                                  disabled={disabled}
                                  className={cn(
                                    "w-full flex items-center justify-between px-3 py-2.5",
                                    "hover:bg-accent/50 transition-colors text-left",
                                    selected && 'bg-primary/5',
                                    disabled && 'opacity-50 cursor-not-allowed'
                                  )}
                                >
                                  <div className="flex items-center gap-2 flex-1">
                                    {multiple && (
                                      <div className={cn(
                                        "w-4 h-4 rounded border flex items-center justify-center flex-shrink-0",
                                        selected ? 'bg-primary border-primary' : 'border-border'
                                      )}>
                                        {selected && <Check className="h-3 w-3 text-white" />}
                                      </div>
                                    )}
                                    <div className="flex-1">
                                      <p className="text-sm font-medium">{infoProp.name}</p>
                                      <p className="text-xs text-muted-foreground">
                                        {infoProp.description} · 当前: {getPreviewValue()}
                                      </p>
                                    </div>
                                  </div>
                                  {!multiple && selected && (
                                    <Check className="h-4 w-4 text-primary flex-shrink-0" />
                                  )}
                                </button>
                              )
                            })}
                          </div>
                        )}
                      </div>
                    )
                  })}
                </div>
              ) : (
                <div className="text-center py-10 text-muted-foreground">
                  <Info className="h-10 w-10 mx-auto mb-3 opacity-40" />
                  <p className="text-sm">暂无设备</p>
                </div>
              )}
            </TabsContent>

            {/* Agent Data Content */}
            <TabsContent value="agent" className="flex-1 overflow-y-auto px-6 py-4 mt-0">
              <div className="space-y-2">
                {AGENT_METRICS.map(metric => {
                  if (!filterMatches(metric.name) && !filterMatches(metric.description)) return null

                  const itemId = `agent:${metric.id}`
                  const selected = isSelected(itemId)
                  const disabled = multiple && !selected && !canSelectMore

                  return (
                    <button
                      key={metric.id}
                      onClick={() => !disabled && handleItemClick(itemId)}
                      disabled={disabled}
                      className={cn(
                        "w-full flex items-center justify-between p-3 rounded-lg border transition-all",
                        selected ? 'bg-primary/10 border-primary/30' : 'bg-card border-border hover:bg-accent/50 hover:border-primary/30',
                        disabled && 'opacity-50 cursor-not-allowed'
                      )}
                    >
                      <div className="flex items-center gap-3">
                        {multiple && (
                          <div className={cn(
                            "w-5 h-5 rounded border flex items-center justify-center transition-colors",
                            selected ? 'bg-primary border-primary' : 'border-border'
                          )}>
                            {selected && <Check className="h-3.5 w-3.5 text-white" />}
                          </div>
                        )}
                        <div className="w-8 h-8 rounded-lg flex items-center justify-center bg-purple-500/10">
                          <Bot className={cn("h-4 w-4", selected ? 'text-primary' : 'text-purple-500')} />
                        </div>
                        <div className="text-left">
                          <p className="text-sm font-medium">{metric.name}</p>
                          <p className="text-xs text-muted-foreground">{metric.description}</p>
                        </div>
                      </div>
                      {!multiple && selected && (
                        <Check className="h-4 w-4 text-primary" />
                      )}
                    </button>
                  )
                })}
              </div>
            </TabsContent>

            {/* System Stats Content */}
            <TabsContent value="system" className="flex-1 overflow-y-auto px-6 py-4 mt-0">
              <div className="space-y-2">
                {SYSTEM_METRICS.map(metric => {
                  if (!filterMatches(metric.name)) return null

                  const itemId = `system:${metric.id}`
                  const selected = isSelected(itemId)
                  const disabled = multiple && !selected && !canSelectMore

                  return (
                    <button
                      key={metric.id}
                      onClick={() => !disabled && handleItemClick(itemId)}
                      disabled={disabled}
                      className={cn(
                        "w-full flex items-center justify-between p-3 rounded-lg border transition-all",
                        selected ? 'bg-primary/10 border-primary/30' : 'bg-card border-border hover:bg-accent/50 hover:border-primary/30',
                        disabled && 'opacity-50 cursor-not-allowed'
                      )}
                    >
                      <div className="flex items-center gap-3">
                        {multiple && (
                          <div className={cn(
                            "w-5 h-5 rounded border flex items-center justify-center transition-colors",
                            selected ? 'bg-primary border-primary' : 'border-border'
                          )}>
                            {selected && <Check className="h-3.5 w-3.5 text-white" />}
                          </div>
                        )}
                        <div className="w-8 h-8 rounded-lg flex items-center justify-center bg-blue-500/10">
                          <BarChart3 className={cn("h-4 w-4", selected ? 'text-primary' : 'text-blue-500')} />
                        </div>
                        <div className="text-left">
                          <p className="text-sm font-medium">{metric.name}</p>
                          <p className="text-xs text-muted-foreground">{metric.unit}</p>
                        </div>
                      </div>
                      {!multiple && selected && (
                        <Check className="h-4 w-4 text-primary" />
                      )}
                    </button>
                  )
                })}
              </div>
            </TabsContent>
          </Tabs>
        </div>

        {/* Footer */}
        <div className="px-6 py-4 border-t flex justify-between items-center shrink-0">
          <Button variant="ghost" onClick={onClose} className="h-9">
            取消
          </Button>
          <Button
            onClick={handleSelect}
            disabled={selectedItems.size === 0}
            className="h-9"
          >
            确认 {totalSelected > 0 && `(${totalSelected})`}
          </Button>
        </div>
      </DialogContent>
    </Dialog>
  )
}
