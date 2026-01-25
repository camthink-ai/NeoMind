/**
 * DataSourceSidebar Component
 *
 * Collapsible sidebar for data source selection in the component config dialog.
 * Collapsed state shows current selection summary, expanded state shows full selector.
 */

import { useState, useEffect, useRef, useMemo } from 'react'
import { Search, Server, Check, Zap, ChevronRight, ChevronLeft, BarChart3, Bot, Info, Layers, Database } from 'lucide-react'
import { Input } from '@/components/ui/input'
import { Button } from '@/components/ui/button'
import { cn } from '@/lib/utils'
import { useStore } from '@/store'
import type { DataSource, DataSourceOrList } from '@/types/dashboard'
import { normalizeDataSource } from '@/types/dashboard'
import type { MetricDefinition, CommandDefinition } from '@/types'

export interface DataSourceSidebarProps {
  open: boolean
  onToggle: () => void
  onSelect: (dataSource: DataSourceOrList | DataSource | undefined) => void
  currentDataSource?: DataSourceOrList
  allowedTypes?: Array<'device-metric' | 'device-command' | 'device-info' | 'agent' | 'system' | 'device' | 'metric' | 'command'>
  multiple?: boolean
  maxSources?: number
  className?: string
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
  { id: 'device-online-count', name: '在线设备数', unit: '个' },
  { id: 'device-offline-count', name: '离线设备数', unit: '个' },
  { id: 'alert-critical-count', name: '严重告警', unit: '个' },
  { id: 'alert-warning-count', name: '警告告警', unit: '个' },
  { id: 'rule-trigger-count', name: '规则触发次数', unit: '次' },
]

// Agent data options
const AGENT_METRICS = [
  { id: 'agent-status', name: 'Agent 状态', description: 'idle, running, paused' },
  { id: 'agent-executions', name: '执行次数', description: '总执行次数' },
  { id: 'agent-success-rate', name: '成功率', description: '执行成功率百分比' },
]

/**
 * Convert selected items to DataSource format
 */
function selectedItemsToDataSource(
  selectedItems: Set<SelectedItem>,
  multiple: boolean
): DataSourceOrList | DataSource | undefined {
  if (selectedItems.size === 0) return undefined

  const items = Array.from(selectedItems)

  if (!multiple && items.length === 1) {
    // Single selection - return single DataSource
    const item = items[0]
    const [type, ...parts] = item.split(':')

    if (type === 'device-metric' && parts.length >= 2) {
      return { type: 'device', deviceId: parts[0], property: parts[1] }
    } else if (type === 'device-command' && parts.length >= 2) {
      return { type: 'command', deviceId: parts[0], command: parts[1] }
    } else if (type === 'device-info' && parts.length >= 2) {
      return { type: 'device-info', deviceId: parts[0], infoProperty: parts[1] as any }
    } else if (type === 'system') {
      const metricId = parts[0]
      return { type: 'api', endpoint: `/api/stats/${metricId.split('-')[0]}s` }
    }
  }

  // Multiple selection - return array
  const sources: DataSource[] = []

  for (const item of items) {
    const [type, ...parts] = item.split(':')

    if (type === 'device-metric' && parts.length >= 2) {
      sources.push({ type: 'device', deviceId: parts[0], property: parts[1] })
    } else if (type === 'device-command' && parts.length >= 2) {
      sources.push({ type: 'command', deviceId: parts[0], command: parts[1] })
    } else if (type === 'device-info' && parts.length >= 2) {
      sources.push({ type: 'device-info', deviceId: parts[0], infoProperty: parts[1] as any })
    } else if (type === 'system') {
      sources.push({ type: 'api', endpoint: `/api/stats/${parts[0].split('-')[0]}s` })
    }
  }

  return sources.length > 0 ? (multiple ? sources : sources[0]) : undefined
}

export function DataSourceSidebar({
  open,
  onToggle,
  onSelect,
  currentDataSource,
  allowedTypes,
  multiple = false,
  maxSources = 10,
  className,
}: DataSourceSidebarProps) {
  const { devices, deviceTypes, fetchDeviceTypes, fetchDevices } = useStore()
  const [searchQuery, setSearchQuery] = useState('')
  const [selectedCategory, setSelectedCategory] = useState<CategoryType>('device-metric')
  const [selectedItems, setSelectedItems] = useState<Set<SelectedItem>>(new Set())
  const [expandedDevices, setExpandedDevices] = useState<Set<string>>(new Set())

  // Track initialization
  const initializedRef = useRef(false)
  const prevOpenRef = useRef(false)

  // Fetch device types and devices when sidebar opens
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

  // Initialize selection when sidebar opens
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
        ]
        map.set(device.id, fallbackMetrics)
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
        // Fallback commands
        const fallbackCommands: CommandDefinition[] = [
          { name: 'toggle', display_name: '切换', parameters: [] },
          { name: 'setValue', display_name: '设置值', parameters: [{ name: 'value', data_type: 'number' }] },
        ]
        map.set(device.id, fallbackCommands)
      }
    }
    return map
  }, [devices, deviceTypes])

  // Toggle device expansion
  const toggleDevice = (deviceId: string) => {
    setExpandedDevices(prev => {
      const next = new Set(prev)
      if (next.has(deviceId)) {
        next.delete(deviceId)
      } else {
        next.add(deviceId)
      }
      return next
    })
  }

  // Handle item selection
  const handleSelectItem = (itemKey: SelectedItem) => {
    setSelectedItems(prev => {
      const next = new Set(prev)

      if (multiple) {
        if (next.has(itemKey)) {
          next.delete(itemKey)
        } else if (next.size < maxSources) {
          next.add(itemKey)
        }
      } else {
        // Single selection - clear others and select this one
        next.clear()
        next.add(itemKey)
      }

      // Auto-apply selection
      const dataSource = selectedItemsToDataSource(next, multiple)
      onSelect(dataSource)

      return next
    })
  }

  // Get current category config
  const categoryConfig = CATEGORIES.find(c => c.id === selectedCategory)
  const CategoryIcon = categoryConfig?.icon || Server

  // Filter devices by search query
  const filteredDevices = useMemo(() => {
    if (!searchQuery) return devices

    const query = searchQuery.toLowerCase()
    return devices.filter(d =>
      d.id.toLowerCase().includes(query) ||
      (d.name && d.name.toLowerCase().includes(query))
    )
  }, [devices, searchQuery])

  // Current selection count
  const selectionCount = selectedItems.size

  return (
    <div className={cn(
      'flex flex-col h-full border-l bg-muted/20 transition-all duration-300 ease-in-out',
      open ? 'flex-1 min-w-[280px]' : 'w-12',
      className
    )}>
      {/* Header */}
      <div className="flex items-center justify-between px-2 py-2 border-b bg-muted/30">
        {open ? (
          <div className="flex items-center gap-2">
            <Database className="h-4 w-4 text-muted-foreground" />
            <span className="text-sm font-medium">数据源</span>
            {selectionCount > 0 && (
              <span className="text-xs bg-primary text-primary-foreground px-1.5 py-0.5 rounded-full">
                {selectionCount}
              </span>
            )}
          </div>
        ) : (
          <span className="text-xs text-muted-foreground/60 px-1 writing-mode-vertical">
            数据源
          </span>
        )}
        <Button
          variant="ghost"
          size="icon"
          className="h-7 w-7 shrink-0"
          onClick={onToggle}
        >
          {open ? (
            <ChevronLeft className="h-4 w-4" />
          ) : (
            <ChevronRight className="h-4 w-4" />
          )}
        </Button>
      </div>

      {/* Content */}
      {open ? (
        <div className="flex-1 overflow-hidden flex flex-col">
          {/* Search */}
          <div className="p-2 border-b">
            <div className="relative">
              <Search className="absolute left-2 top-1/2 -translate-y-1/2 h-3.5 w-3.5 text-muted-foreground" />
              <Input
                placeholder="搜索设备..."
                value={searchQuery}
                onChange={(e) => setSearchQuery(e.target.value)}
                className="h-8 pl-8 text-xs"
              />
            </div>
          </div>

          {/* Category Tabs */}
          <div className="flex border-b overflow-x-auto">
            {availableCategories.map(cat => {
              const Icon = cat.icon
              return (
                <button
                  key={cat.id}
                  onClick={() => setSelectedCategory(cat.id)}
                  className={cn(
                    'flex items-center gap-1 px-2 py-1.5 text-xs whitespace-nowrap transition-colors border-b-2',
                    selectedCategory === cat.id
                      ? 'border-primary text-primary bg-primary/5'
                      : 'border-transparent text-muted-foreground hover:text-foreground hover:bg-muted/50'
                  )}
                >
                  <Icon className="h-3 w-3" />
                  <span>{cat.name}</span>
                </button>
              )
            })}
          </div>

          {/* Category Content */}
          <div className="flex-1 overflow-y-auto p-2">
            {selectedCategory === 'device-metric' && (
              <div className="space-y-1">
                {filteredDevices.map(device => {
                  const metrics = deviceMetricsMap.get(device.id) || []
                  const isExpanded = expandedDevices.has(device.id)

                  return (
                    <div key={device.id} className="border rounded-md overflow-hidden">
                      <button
                        onClick={() => toggleDevice(device.id)}
                        className="w-full flex items-center justify-between px-2 py-1.5 hover:bg-muted/50 transition-colors"
                      >
                        <span className="text-xs font-medium truncate">{device.name || device.id}</span>
                        <ChevronRight className={cn(
                          'h-3 w-3 transition-transform',
                          isExpanded && 'rotate-90'
                        )} />
                      </button>
                      {isExpanded && metrics.map(metric => {
                        const itemKey = `device-metric:${device.id}:${metric.name}` as SelectedItem
                        const isSelected = selectedItems.has(itemKey)

                        return (
                          <button
                            key={metric.name}
                            onClick={() => handleSelectItem(itemKey)}
                            className={cn(
                              'w-full flex items-center justify-between px-3 py-1.5 text-xs hover:bg-muted/50 transition-colors',
                              isSelected && 'bg-primary/10'
                            )}
                          >
                            <span>{metric.display_name || metric.name}</span>
                            {isSelected && <Check className="h-3 w-3 text-primary" />}
                          </button>
                        )
                      })}
                    </div>
                  )
                })}
              </div>
            )}

            {selectedCategory === 'device-command' && (
              <div className="space-y-1">
                {filteredDevices.map(device => {
                  const commands = deviceCommandsMap.get(device.id) || []
                  const isExpanded = expandedDevices.has(device.id)

                  return (
                    <div key={device.id} className="border rounded-md overflow-hidden">
                      <button
                        onClick={() => toggleDevice(device.id)}
                        className="w-full flex items-center justify-between px-2 py-1.5 hover:bg-muted/50 transition-colors"
                      >
                        <span className="text-xs font-medium truncate">{device.name || device.id}</span>
                        <ChevronRight className={cn(
                          'h-3 w-3 transition-transform',
                          isExpanded && 'rotate-90'
                        )} />
                      </button>
                      {isExpanded && commands.map(command => {
                        const itemKey = `device-command:${device.id}:${command.name}` as SelectedItem
                        const isSelected = selectedItems.has(itemKey)

                        return (
                          <button
                            key={command.name}
                            onClick={() => handleSelectItem(itemKey)}
                            className={cn(
                              'w-full flex items-center justify-between px-3 py-1.5 text-xs hover:bg-muted/50 transition-colors',
                              isSelected && 'bg-primary/10'
                            )}
                          >
                            <span>{command.display_name || command.name}</span>
                            {isSelected && <Check className="h-3 w-3 text-primary" />}
                          </button>
                        )
                      })}
                    </div>
                  )
                })}
              </div>
            )}

            {selectedCategory === 'device-info' && (
              <div className="space-y-1">
                {filteredDevices.map(device => {
                  const isExpanded = expandedDevices.has(device.id)

                  return (
                    <div key={device.id} className="border rounded-md overflow-hidden">
                      <button
                        onClick={() => toggleDevice(device.id)}
                        className="w-full flex items-center justify-between px-2 py-1.5 hover:bg-muted/50 transition-colors"
                      >
                        <span className="text-xs font-medium truncate">{device.name || device.id}</span>
                        <ChevronRight className={cn(
                          'h-3 w-3 transition-transform',
                          isExpanded && 'rotate-90'
                        )} />
                      </button>
                      {isExpanded && DEVICE_INFO_PROPERTIES.map(prop => {
                        const itemKey = `device-info:${device.id}:${prop.id}` as SelectedItem
                        const isSelected = selectedItems.has(itemKey)

                        return (
                          <button
                            key={prop.id}
                            onClick={() => handleSelectItem(itemKey)}
                            className={cn(
                              'w-full flex items-center justify-between px-3 py-1.5 text-xs hover:bg-muted/50 transition-colors',
                              isSelected && 'bg-primary/10'
                            )}
                          >
                            <span>{prop.name}</span>
                            {isSelected && <Check className="h-3 w-3 text-primary" />}
                          </button>
                        )
                      })}
                    </div>
                  )
                })}
              </div>
            )}

            {selectedCategory === 'agent' && (
              <div className="space-y-1">
                {AGENT_METRICS.map(metric => {
                  const itemKey = `agent:${metric.id}` as SelectedItem
                  const isSelected = selectedItems.has(itemKey)

                  return (
                    <button
                      key={metric.id}
                      onClick={() => handleSelectItem(itemKey)}
                      className={cn(
                        'w-full flex items-center justify-between px-3 py-2 text-xs hover:bg-muted/50 transition-colors rounded-md',
                        isSelected && 'bg-primary/10'
                      )}
                    >
                      <div className="text-left">
                        <div className="font-medium">{metric.name}</div>
                        <div className="text-muted-foreground text-[10px]">{metric.description}</div>
                      </div>
                      {isSelected && <Check className="h-3 w-3 text-primary shrink-0" />}
                    </button>
                  )
                })}
              </div>
            )}

            {selectedCategory === 'system' && (
              <div className="space-y-1">
                {SYSTEM_METRICS.map(metric => {
                  const itemKey = `system:${metric.id}` as SelectedItem
                  const isSelected = selectedItems.has(itemKey)

                  return (
                    <button
                      key={metric.id}
                      onClick={() => handleSelectItem(itemKey)}
                      className={cn(
                        'w-full flex items-center justify-between px-3 py-2 text-xs hover:bg-muted/50 transition-colors rounded-md',
                        isSelected && 'bg-primary/10'
                      )}
                    >
                      <div className="text-left">
                        <div className="font-medium">{metric.name}</div>
                        <div className="text-muted-foreground text-[10px]">单位: {metric.unit}</div>
                      </div>
                      {isSelected && <Check className="h-3 w-3 text-primary shrink-0" />}
                    </button>
                  )
                })}
              </div>
            )}
          </div>
        </div>
      ) : (
        /* Collapsed state - show selection indicator */
        <div className="flex-1 flex items-center justify-center">
          {selectionCount > 0 ? (
            <div className="text-center">
              <div className="relative">
                <Database className="h-5 w-5 text-primary mx-auto" />
                <span className="absolute -top-1 -right-1 h-4 w-4 bg-primary text-primary-foreground text-[10px] flex items-center justify-center rounded-full">
                  {selectionCount}
                </span>
              </div>
            </div>
          ) : (
            <Database className="h-5 w-5 text-muted-foreground/40" />
          )}
        </div>
      )}
    </div>
  )
}
