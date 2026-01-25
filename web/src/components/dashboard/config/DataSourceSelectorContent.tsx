/**
 * DataSourceSelectorContent Component
 *
 * Content-only version of the data source selector for use in dialogs/sheets.
 * Two-panel layout: device list on left, details on right.
 * Shows selected items with individual remove buttons.
 */

import { useState, useMemo, useEffect } from 'react'
import { Search, Server, Check, Zap, BarChart3, Bot, Info, ChevronRight, X, ChevronDown } from 'lucide-react'
import { Input } from '@/components/ui/input'
import { cn } from '@/lib/utils'
import { useStore } from '@/store'
import type { DataSource, DataSourceOrList } from '@/types/dashboard'
import { normalizeDataSource } from '@/types/dashboard'
import type { MetricDefinition, CommandDefinition } from '@/types'

export interface DataSourceSelectorContentProps {
  onSelect: (dataSource: DataSourceOrList | DataSource | undefined) => void
  currentDataSource?: DataSourceOrList
  allowedTypes?: Array<'device-metric' | 'device-command' | 'device-info' | 'agent' | 'system' | 'device' | 'metric' | 'command'>
  multiple?: boolean
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
  if (!multiple && selectedItems.size === 1) {
    const item = [...selectedItems][0]!
    const [type, deviceId, ...rest] = item.split(':')

    switch (type as CategoryType) {
      case 'device-metric':
        // Return telemetry type for chart components to fetch historical data
        return {
          type: 'telemetry',
          deviceId,
          metricId: rest.join(':'),
          timeRange: 1, // Default 1 hour
          limit: 50,    // Default 50 points
          aggregate: 'raw',
          params: {
            includeRawPoints: true,
          },
          transform: 'raw',
        }
      case 'device-command':
        return {
          type: 'command',
          deviceId,
          command: rest.join(':'),
        }
      case 'device-info':
        return {
          type: 'device-info',
          deviceId,
          property: rest.join(':'),
        }
      case 'agent':
        return {
          type: 'metric',
          metricId: rest.join(':'),
        }
      case 'system':
        return {
          type: 'metric',
          metricId: rest.join(':'),
        }
      default:
        return undefined
    }
  }

  // Multiple selection - return array
  const result: DataSource[] = []
  for (const item of selectedItems) {
    const [type, deviceId, ...rest] = item.split(':')

    switch (type as CategoryType) {
      case 'device-metric':
        // Return telemetry type for chart components to fetch historical data
        result.push({
          type: 'telemetry',
          deviceId,
          metricId: rest.join(':'),
          timeRange: 1,
          limit: 50,
          aggregate: 'raw',
          params: {
            includeRawPoints: true,
          },
          transform: 'raw',
        })
        break
      case 'device-command':
        result.push({
          type: 'command',
          deviceId,
          command: rest.join(':'),
        })
        break
      case 'device-info':
        result.push({
          type: 'device-info',
          deviceId,
          property: rest.join(':'),
        })
        break
      case 'agent':
        result.push({
          type: 'metric',
          metricId: rest.join(':'),
        })
        break
      case 'system':
        result.push({
          type: 'metric',
          metricId: rest.join(':'),
        })
        break
    }
  }

  return result
}

/**
 * Parse current data source to selected items
 */
function dataSourceToSelectedItems(ds: DataSourceOrList | undefined): Set<SelectedItem> {
  const items = new Set<SelectedItem>()
  if (!ds) return items

  const dataSources = normalizeDataSource(ds)

  for (const dataSource of dataSources) {
    switch (dataSource.type) {
      case 'device':
        items.add(`device-metric:${dataSource.deviceId}:${dataSource.property}` as SelectedItem)
        break
      case 'telemetry':
        // Telemetry type is used for historical device metric data
        items.add(`device-metric:${dataSource.deviceId}:${dataSource.metricId}` as SelectedItem)
        break
      case 'command':
        items.add(`device-command:${dataSource.deviceId}:${dataSource.command}` as SelectedItem)
        break
      case 'device-info':
        items.add(`device-info:${dataSource.deviceId}:${dataSource.property}` as SelectedItem)
        break
      case 'metric':
        // Determine if this is an agent or system metric
        if (dataSource.metricId?.startsWith('agent-')) {
          items.add(`agent:${dataSource.metricId}` as SelectedItem)
        } else {
          items.add(`system:${dataSource.metricId}` as SelectedItem)
        }
        break
    }
  }

  return items
}

/**
 * Get a readable label for a selected item
 */
function getSelectedItemLabel(item: SelectedItem, devices: any[]): string {
  const [type, deviceId, ...rest] = item.split(':')

  switch (type as CategoryType) {
    case 'device-metric':
      const device1 = devices.find(d => d.id === deviceId)
      const deviceName1 = device1?.name || deviceId
      return `${deviceName1} · ${rest.join(':')}`
    case 'device-command':
      const device2 = devices.find(d => d.id === deviceId)
      const deviceName2 = device2?.name || deviceId
      return `${deviceName2} · ${rest.join(':')}`
    case 'device-info':
      const device3 = devices.find(d => d.id === deviceId)
      const deviceName3 = device3?.name || deviceId
      const prop = DEVICE_INFO_PROPERTIES.find(p => p.id === rest.join(':'))
      return `${deviceName3} · ${prop?.name || rest.join(':')}`
    case 'agent':
      const agentMetric = AGENT_METRICS.find(m => m.id === rest.join(':'))
      return agentMetric?.name || rest.join(':')
    case 'system':
      const sysMetric = SYSTEM_METRICS.find(m => m.id === rest.join(':'))
      return sysMetric?.name || rest.join(':')
    default:
      return item
  }
}

export function DataSourceSelectorContent({
  onSelect,
  currentDataSource,
  allowedTypes,
  multiple = false,
  maxSources = 10,
}: DataSourceSelectorContentProps) {
  const { devices, deviceTypes } = useStore()

  const [searchQuery, setSearchQuery] = useState('')
  const [selectedCategory, setSelectedCategory] = useState<CategoryType>('device-metric')
  const [selectedDeviceId, setSelectedDeviceId] = useState<string | null>(null)
  const [showSelectedItems, setShowSelectedItems] = useState(true)

  // Initialize selected items from current data source
  const [selectedItems, setSelectedItems] = useState<Set<SelectedItem>>(() =>
    dataSourceToSelectedItems(currentDataSource)
  )

  // Sync selected items when currentDataSource prop changes (e.g., when cleared from outside)
  useEffect(() => {
    setSelectedItems(dataSourceToSelectedItems(currentDataSource))
  }, [currentDataSource])

  // Available categories based on allowedTypes
  const availableCategories = useMemo(
    () => CATEGORIES.filter(c => normalizeAllowedTypes(allowedTypes).includes(c.id)),
    [allowedTypes]
  )

  // Set initial category to first available
  useMemo(() => {
    if (availableCategories.length > 0 && !availableCategories.find(c => c.id === selectedCategory)) {
      setSelectedCategory(availableCategories[0]!.id)
    }
  }, [availableCategories, selectedCategory])

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

  // Handle item selection
  const handleSelectItem = (itemKey: SelectedItem) => {
    setSelectedItems(prev => {
      const next = new Set(prev)

      if (multiple) {
        // Multiple selection mode - toggle
        if (next.has(itemKey)) {
          next.delete(itemKey)
        } else if (next.size < maxSources) {
          next.add(itemKey)
        }
      } else {
        // Single selection mode
        if (next.has(itemKey)) {
          // Clicking the same item deselects it (clears selection)
          next.clear()
        } else {
          // Select new item, clearing others
          next.clear()
          next.add(itemKey)
        }
      }

      // Auto-apply selection
      const dataSource = selectedItemsToDataSource(next, multiple)
      onSelect(dataSource)

      return next
    })
  }

  // Remove a specific selected item
  const handleRemoveItem = (itemKey: SelectedItem) => {
    setSelectedItems(prev => {
      const next = new Set(prev)
      next.delete(itemKey)

      const dataSource = selectedItemsToDataSource(next, multiple)
      onSelect(dataSource)

      return next
    })
  }

  // Clear all selections
  const handleClearSelection = () => {
    setSelectedItems(new Set())
    onSelect(undefined)
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

  // Check if current category is device-based (has device list)
  const isDeviceBasedCategory = selectedCategory === 'device-metric' ||
                                selectedCategory === 'device-command' ||
                                selectedCategory === 'device-info'

  // Auto-select first device when switching to device-based category
  useMemo(() => {
    if (isDeviceBasedCategory && !selectedDeviceId && filteredDevices.length > 0) {
      setSelectedDeviceId(filteredDevices[0]!.id)
    }
  }, [isDeviceBasedCategory, selectedDeviceId, filteredDevices])

  // Convert selected items to array for display
  const selectedItemsArray = useMemo(() => Array.from(selectedItems), [selectedItems])

  return (
    <div className="flex flex-col h-full">
      {/* Search & Actions */}
      <div className="p-3 border-b shrink-0 flex gap-2">
        <div className="relative flex-1">
          <Search className="absolute left-3 top-1/2 -translate-y-1/2 h-4 w-4 text-muted-foreground" />
          <Input
            placeholder="搜索设备..."
            value={searchQuery}
            onChange={(e) => setSearchQuery(e.target.value)}
            className="h-8 pl-9 text-xs"
          />
        </div>
        {selectedItems.size > 0 && (
          <button
            onClick={handleClearSelection}
            className="h-8 px-2 text-xs flex items-center gap-1 text-muted-foreground hover:text-destructive hover:bg-destructive/10 rounded-md transition-colors shrink-0"
            title="清除所有选择"
          >
            <X className="h-3.5 w-3.5" />
            <span className="hidden sm:inline">清除全部</span>
          </button>
        )}
      </div>

      {/* Selected Items Panel (shown when there are selections in multiple mode) */}
      {multiple && selectedItems.size > 0 && (
        <div className="px-3 py-2 border-b bg-muted/20 shrink-0">
          <button
            onClick={() => setShowSelectedItems(!showSelectedItems)}
            className="flex items-center justify-between w-full text-xs text-muted-foreground hover:text-foreground transition-colors"
          >
            <span className="flex items-center gap-1.5">
              <Check className="h-3.5 w-3.5 text-primary" />
              <span className="font-medium">已选 {selectedItems.size} 项</span>
            </span>
            <ChevronDown className={cn(
              'h-3.5 w-3.5 transition-transform',
              showSelectedItems ? 'rotate-180' : ''
            )} />
          </button>
          {showSelectedItems && (
            <div className="mt-2 flex flex-wrap gap-1.5">
              {selectedItemsArray.map(item => {
                const label = getSelectedItemLabel(item, devices)
                return (
                  <span
                    key={item}
                    className="inline-flex items-center gap-1 px-2 py-1 bg-primary/10 text-primary rounded-md text-xs"
                  >
                    <span className="max-w-[120px] truncate">{label}</span>
                    <button
                      onClick={() => handleRemoveItem(item)}
                      className="hover:text-destructive transition-colors"
                      title="移除"
                    >
                      <X className="h-3 w-3" />
                    </button>
                  </span>
                )
              })}
            </div>
          )}
        </div>
      )}

      {/* Category Tabs */}
      <div className="flex border-b shrink-0 overflow-x-auto">
        {availableCategories.map(cat => {
          const Icon = cat.icon
          return (
            <button
              key={cat.id}
              onClick={() => {
                setSelectedCategory(cat.id)
                setSelectedDeviceId(null)
              }}
              className={cn(
                'flex items-center gap-1 px-2 py-2 text-xs whitespace-nowrap transition-colors border-b-2 shrink-0',
                selectedCategory === cat.id
                  ? 'border-primary text-primary bg-primary/5'
                  : 'border-transparent text-muted-foreground hover:text-foreground hover:bg-muted/50'
              )}
            >
              <Icon className="h-3.5 w-3.5" />
              <span>{cat.name}</span>
            </button>
          )
        })}
      </div>

      {/* Two-panel Content Area */}
      <div className="flex-1 flex min-h-0">
        {/* Device-based categories: two-panel layout */}
        {isDeviceBasedCategory ? (
          <>
            {/* Left: Device List */}
            <div className="w-[180px] border-r shrink-0 overflow-y-auto">
              <div className="p-1 space-y-0.5">
                {filteredDevices.length === 0 ? (
                  <div className="text-center py-4 text-muted-foreground">
                    <p className="text-xs">没有设备</p>
                  </div>
                ) : (
                  filteredDevices.map(device => {
                    // Count how many items from this device are selected
                    const deviceSelectedCount = selectedItemsArray.filter(item =>
                      item.startsWith(`device-metric:${device.id}:`) ||
                      item.startsWith(`device-command:${device.id}:`) ||
                      item.startsWith(`device-info:${device.id}:`)
                    ).length

                    const isSelected = selectedDeviceId === device.id

                    return (
                      <button
                        key={device.id}
                        onClick={() => setSelectedDeviceId(device.id)}
                        className={cn(
                          'w-full flex items-center gap-2 px-2 py-1.5 text-xs text-left rounded-md transition-colors',
                          isSelected ? 'bg-primary text-primary-foreground' : 'hover:bg-muted/50'
                        )}
                      >
                        <div className="flex-1 min-w-0 truncate">
                          {device.name || device.id}
                        </div>
                        {deviceSelectedCount > 0 && !isSelected && (
                          <span className="text-[10px] bg-primary/20 text-primary px-1 rounded-full shrink-0">
                            {deviceSelectedCount}
                          </span>
                        )}
                        {isSelected && <ChevronRight className="h-3 w-3 shrink-0" />}
                      </button>
                    )
                  })
                )}
              </div>
            </div>

            {/* Right: Details Panel */}
            <div className="flex-1 overflow-y-auto">
              {!selectedDeviceId ? (
                <div className="flex items-center justify-center h-full text-muted-foreground">
                  <p className="text-xs">选择一个设备</p>
                </div>
              ) : (
                <div className="p-2">
                  {selectedCategory === 'device-metric' && (
                    <div className="space-y-0.5">
                      {(deviceMetricsMap.get(selectedDeviceId) || []).map(metric => {
                        const itemKey = `device-metric:${selectedDeviceId}:${metric.name}` as SelectedItem
                        const isSelected = selectedItems.has(itemKey)

                        return (
                          <button
                            key={metric.name}
                            onClick={() => handleSelectItem(itemKey)}
                            className={cn(
                              'w-full flex items-center justify-between px-3 py-1.5 text-xs hover:bg-muted/50 rounded-md transition-colors',
                              isSelected && 'bg-primary/10 text-primary'
                            )}
                          >
                            <span>{metric.display_name || metric.name}</span>
                            {isSelected && <Check className="h-3.5 w-3.5" />}
                          </button>
                        )
                      })}
                    </div>
                  )}

                  {selectedCategory === 'device-command' && (
                    <div className="space-y-0.5">
                      {(deviceCommandsMap.get(selectedDeviceId) || []).map(command => {
                        const itemKey = `device-command:${selectedDeviceId}:${command.name}` as SelectedItem
                        const isSelected = selectedItems.has(itemKey)

                        return (
                          <button
                            key={command.name}
                            onClick={() => handleSelectItem(itemKey)}
                            className={cn(
                              'w-full flex items-center justify-between px-3 py-1.5 text-xs hover:bg-muted/50 rounded-md transition-colors',
                              isSelected && 'bg-primary/10 text-primary'
                            )}
                          >
                            <span>{command.display_name || command.name}</span>
                            {isSelected && <Check className="h-3.5 w-3.5" />}
                          </button>
                        )
                      })}
                    </div>
                  )}

                  {selectedCategory === 'device-info' && (
                    <div className="space-y-0.5">
                      {DEVICE_INFO_PROPERTIES.map(prop => {
                        const itemKey = `device-info:${selectedDeviceId}:${prop.id}` as SelectedItem
                        const isSelected = selectedItems.has(itemKey)

                        return (
                          <button
                            key={prop.id}
                            onClick={() => handleSelectItem(itemKey)}
                            className={cn(
                              'w-full flex items-center justify-between px-3 py-1.5 text-xs hover:bg-muted/50 rounded-md transition-colors',
                              isSelected && 'bg-primary/10 text-primary'
                            )}
                          >
                            <div className="text-left">
                              <div className="font-medium">{prop.name}</div>
                              <div className="text-[10px] text-muted-foreground">{prop.description}</div>
                            </div>
                            {isSelected && <Check className="h-3.5 w-3.5 shrink-0" />}
                          </button>
                        )
                      })}
                    </div>
                  )}
                </div>
              )}
            </div>
          </>
        ) : (
          /* Non-device categories: single panel */
          <div className="flex-1 overflow-y-auto p-2">
            {selectedCategory === 'agent' && (
              <div className="space-y-0.5">
                {AGENT_METRICS.map(metric => {
                  const itemKey = `agent:${metric.id}` as SelectedItem
                  const isSelected = selectedItems.has(itemKey)

                  return (
                    <button
                      key={metric.id}
                      onClick={() => handleSelectItem(itemKey)}
                      className={cn(
                        'w-full flex items-center justify-between px-3 py-1.5 text-xs hover:bg-muted/50 rounded-md transition-colors',
                        isSelected && 'bg-primary/10 text-primary'
                      )}
                    >
                      <div className="text-left">
                        <div className="font-medium">{metric.name}</div>
                        <div className="text-[10px] text-muted-foreground">{metric.description}</div>
                      </div>
                      {isSelected && <Check className="h-3.5 w-3.5 shrink-0" />}
                    </button>
                  )
                })}
              </div>
            )}

            {selectedCategory === 'system' && (
              <div className="space-y-0.5">
                {SYSTEM_METRICS.map(metric => {
                  const itemKey = `system:${metric.id}` as SelectedItem
                  const isSelected = selectedItems.has(itemKey)

                  return (
                    <button
                      key={metric.id}
                      onClick={() => handleSelectItem(itemKey)}
                      className={cn(
                        'w-full flex items-center justify-between px-3 py-1.5 text-xs hover:bg-muted/50 rounded-md transition-colors',
                        isSelected && 'bg-primary/10 text-primary'
                      )}
                    >
                      <div className="text-left">
                        <div className="font-medium">{metric.name}</div>
                        <div className="text-[10px] text-muted-foreground">单位: {metric.unit}</div>
                      </div>
                      {isSelected && <Check className="h-3.5 w-3.5 shrink-0" />}
                    </button>
                  )
                })}
              </div>
            )}
          </div>
        )}
      </div>
    </div>
  )
}
