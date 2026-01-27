/**
 * Layer Editor Dialog
 *
 * Custom layer item position editor with:
 * - Left panel: List of bound items (devices/metrics/commands/text/icons)
 * - Right panel: Large interactive layer preview
 * - Click to set item position
 * - Drag to reposition items
 */

import { useState, useCallback, useRef, useEffect } from 'react'
import { Dialog, DialogContent, DialogHeader, DialogTitle, DialogFooter } from '@/components/ui/dialog'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { cn } from '@/lib/utils'
import {
  Trash2,
  GripVertical,
  Check,
  Activity,
  Zap,
  Layers,
  MapPin,
  Type,
  Sparkles,
} from 'lucide-react'
import { CustomLayer, type LayerBinding, type LayerItem } from './CustomLayer'
import { useStore } from '@/store'

// Re-export types for convenience
export type { LayerBinding, LayerItem }

// Type config matching CustomLayer
const TYPE_CONFIG = {
  device: {
    label: '设备',
    icon: MapPin,
    color: 'text-green-600 dark:text-green-400',
    bgColor: 'bg-green-500/20',
  },
  metric: {
    label: '指标',
    icon: Activity,
    color: 'text-purple-600 dark:text-purple-400',
    bgColor: 'bg-purple-500/20',
  },
  command: {
    label: '指令',
    icon: Zap,
    color: 'text-blue-600 dark:text-blue-400',
    bgColor: 'bg-blue-500/20',
  },
  text: {
    label: '文本',
    icon: Type,
    color: 'text-foreground',
    bgColor: 'bg-muted/50',
  },
  icon: {
    label: '图标',
    icon: Sparkles,
    color: 'text-orange-600 dark:text-orange-400',
    bgColor: 'bg-orange-500/20',
  },
} as const

interface LayerEditorDialogProps {
  open: boolean
  onOpenChange: (open: boolean) => void
  bindings: LayerBinding[]
  backgroundType?: 'color' | 'image' | 'transparent' | 'grid'
  backgroundColor?: string
  backgroundImage?: string
  onSave: (bindings: LayerBinding[]) => void
}

export function LayerEditorDialog({
  open,
  onOpenChange,
  bindings: initialBindings,
  backgroundType = 'grid',
  backgroundColor,
  backgroundImage,
  onSave,
}: LayerEditorDialogProps) {
  const [bindings, setBindings] = useState<LayerBinding[]>(initialBindings)
  const [selectedBinding, setSelectedBinding] = useState<string | null>(null)

  // Get devices from store for reactive updates
  const devices = useStore(state => state.devices)

  // Reset state when dialog opens
  useEffect(() => {
    if (open) {
      setBindings(initialBindings)
      setSelectedBinding(null)
    }
  }, [open, initialBindings])

  // Convert bindings to layer items for preview
  const convertToLayerItems = useCallback((): LayerItem[] => {
    const getDeviceName = (deviceId: string) => {
      const device = devices.find(d => d.id === deviceId || d.device_id === deviceId)
      return device?.name || device?.device_id || deviceId
    }

    const getDeviceStatus = (deviceId: string): 'online' | 'offline' | 'error' | 'warning' | undefined => {
      const device = devices.find(d => d.id === deviceId || d.device_id === deviceId)
      if (!device) return undefined
      return device.online ? 'online' : 'offline'
    }

    const getDeviceMetricValue = (deviceId: string, metricId: string): string | number | undefined => {
      const device = devices.find(d => d.id === deviceId || d.device_id === deviceId)
      if (!device?.current_values) return undefined
      const value = device.current_values[metricId || '']
      if (value !== undefined && value !== null) {
        return typeof value === 'number' ? value : String(value)
      }
      return undefined
    }

    return bindings.map((binding): LayerItem => {
      const position = binding.position === 'auto' || !binding.position
        ? { x: 50, y: 50 }
        : binding.position

      const ds = binding.dataSource as any
      const deviceId = ds?.deviceId

      const item: LayerItem = {
        id: binding.id,
        type: binding.type || binding.icon || 'text',
        position,
        label: binding.name,
        color: binding.color,
        backgroundColor: binding.backgroundColor,
        fontSize: binding.fontSize,
        fontWeight: binding.fontWeight,
        visible: true,
        locked: false,
        draggable: true,
      }

      // Set type-specific fields
      if (binding.type === 'metric') {
        item.deviceId = deviceId
        item.metricId = ds?.metricId || ds?.property
        item.deviceName = getDeviceName(deviceId || '')
        item.metricName = ds?.metricId || ds?.property
        const metricValue = getDeviceMetricValue(deviceId || '', item.metricId || '')
        item.value = metricValue !== undefined ? metricValue : '--'
      } else if (binding.type === 'command') {
        item.command = ds?.command
        item.deviceId = deviceId
        item.deviceName = getDeviceName(deviceId || '')
      } else if (binding.type === 'device') {
        item.deviceId = deviceId
        item.deviceName = getDeviceName(deviceId || '')
        item.status = getDeviceStatus(deviceId || '')
      } else if (binding.type === 'text') {
        item.value = ds?.text || ''
      } else if (binding.type === 'icon') {
        item.icon = ds?.icon || ''
      }

      return item
    })
  }, [bindings, devices])

  // Handle removing a binding
  const handleRemoveBinding = useCallback((id: string) => {
    setBindings(bindings.filter(b => b.id !== id))
    if (selectedBinding === id) {
      setSelectedBinding(null)
    }
  }, [bindings, selectedBinding])

  // Handle updating binding position from layer click
  const handleLayerClick = useCallback((x: number, y: number) => {
    if (selectedBinding) {
      setBindings(prev => prev.map(b =>
        b.id === selectedBinding
          ? { ...b, position: { x, y } }
          : b
      ))
    }
  }, [selectedBinding])

  // Handle selecting a binding
  const handleSelectBinding = useCallback((id: string) => {
    setSelectedBinding(id)
  }, [])

  // Handle save
  const handleSave = useCallback(() => {
    onSave(bindings)
    onOpenChange(false)
  }, [bindings, onSave, onOpenChange])

  // Handle items change from layer (drag updates)
  const handleItemsChange = useCallback((items: LayerItem[]) => {
    // Update bindings based on new item positions
    setBindings(prev => prev.map(b => {
      const item = items.find(i => i.id === b.id)
      if (item) {
        return { ...b, position: item.position }
      }
      return b
    }))
  }, [])

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-6xl h-[80vh] p-0 gap-0 flex flex-col">
        <DialogHeader className="px-6 py-4 border-b">
          <DialogTitle className="text-lg">编辑图层项</DialogTitle>
        </DialogHeader>

        <div className="flex-1 flex overflow-hidden">
          {/* Left Panel - Bindings List */}
          <div className="w-80 border-r bg-muted/20 flex flex-col">
            <div className="p-3 border-b bg-muted/30">
              <div className="text-xs font-medium text-muted-foreground uppercase tracking-wide">
                已绑定项目 ({bindings.length})
              </div>
            </div>

            <div className="flex-1 overflow-y-auto p-2 space-y-1">
              {bindings.length === 0 ? (
                <div className="text-center py-8 text-muted-foreground">
                  <Layers className="h-8 w-8 mx-auto mb-2 opacity-50" />
                  <p className="text-sm">暂无项目</p>
                  <p className="text-xs mt-1">请在配置面板中添加数据源</p>
                </div>
              ) : (
                bindings.map((binding) => {
                  const config = TYPE_CONFIG[binding.icon || binding.type]
                  const Icon = config.icon
                  const isSelected = selectedBinding === binding.id

                  return (
                    <div
                      key={binding.id}
                      className={cn(
                        'group flex items-center gap-2 p-2 rounded-lg border transition-all cursor-pointer',
                        isSelected
                          ? 'border-primary bg-primary/5'
                          : 'border-border hover:border-primary/50 hover:bg-muted/50'
                      )}
                      onClick={() => handleSelectBinding(binding.id)}
                    >
                      <GripVertical className="h-4 w-4 text-muted-foreground cursor-grab" />

                      <div className={cn(
                        'w-8 h-8 rounded-full flex items-center justify-center',
                        config.bgColor
                      )}>
                        <Icon className={cn('h-4 w-4', config.color)} />
                      </div>

                      <div className="flex-1 min-w-0">
                        <div className="text-sm font-medium truncate">{binding.name}</div>
                        <div className="text-xs text-muted-foreground truncate">
                          {config.label}
                          {binding.position && binding.position !== 'auto' && (
                            <span> • ({binding.position.x.toFixed(0)}%, {binding.position.y.toFixed(0)}%)</span>
                          )}
                          {binding.position === 'auto' && <span> • 自动定位</span>}
                        </div>
                      </div>

                      <div className="flex items-center gap-1 opacity-0 group-hover:opacity-100 transition-opacity">
                        <Button
                          variant="ghost"
                          size="icon"
                          className="h-6 w-6 text-destructive"
                          onClick={(e) => {
                            e.stopPropagation()
                            handleRemoveBinding(binding.id)
                          }}
                          title="删除"
                        >
                          <Trash2 className="h-3 w-3" />
                        </Button>
                      </div>
                    </div>
                  )
                })
              )}
            </div>
          </div>

          {/* Right Panel - Layer Preview */}
          <div className="flex-1 relative bg-muted/30">
            <div className="absolute inset-0 p-4">
              <CustomLayer
                bindings={bindings}
                backgroundType={backgroundType}
                backgroundColor={backgroundColor}
                backgroundImage={backgroundImage}
                showControls={true}
                showFullscreen={false}
                interactive={true}
                editable={false}
                size="md"
                onItemsChange={handleItemsChange}
                onLayerClick={handleLayerClick}
                className="w-full h-full"
              />
            </div>

            {/* Positioning mode indicator */}
            {selectedBinding && (
              <div className="absolute top-4 left-1/2 -translate-x-1/2 px-3 py-1.5 bg-primary text-primary-foreground rounded-full text-xs font-medium shadow-lg">
                点击图层设置项目位置
              </div>
            )}
          </div>
        </div>

        <DialogFooter className="px-6 py-4 border-t bg-muted/20">
          <Button variant="outline" onClick={() => onOpenChange(false)}>
            取消
          </Button>
          <Button onClick={handleSave}>
            <Check className="h-4 w-4 mr-1" />
            保存更改
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
