/**
 * Layer Editor Dialog
 *
 * Custom layer item position editor with:
 * - Left panel: List of bound items (devices/metrics/commands/text/icons)
 * - Right panel: Large interactive layer preview
 * - Click to set item position
 * - Drag to reposition items
 */

import { useState, useCallback, useEffect } from 'react'
import { findDevice } from '@/lib/deviceUtils'
import { useTranslation } from 'react-i18next'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Slider } from '@/components/ui/slider'
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
  Edit3,
  X,
} from 'lucide-react'
import { CustomLayer, type LayerBinding, type LayerItem } from './CustomLayer'
import { useStore } from '@/store'
import { UnifiedFormDialog } from '@/components/dialog/UnifiedFormDialog'

// Re-export types for convenience
export type { LayerBinding, LayerItem }

// Helper function to find metric value with fuzzy matching (handles nested paths like 'values.image')
function findMetricValue(currentValues: Record<string, unknown> | undefined, metricId: string): unknown {
  if (!currentValues) return undefined

  // 1. Try exact match
  if (metricId in currentValues) {
    return currentValues[metricId]
  }

  // 2. Try case-insensitive match
  const lowerMetricId = metricId.toLowerCase()
  for (const key of Object.keys(currentValues)) {
    if (key.toLowerCase() === lowerMetricId) {
      return currentValues[key]
    }
  }

  // 3. Try nested path like "values.image"
  const parts = metricId.split('.')
  let nested: any = currentValues
  for (const part of parts) {
    if (nested && typeof nested === 'object' && part in nested) {
      nested = nested[part]
    } else {
      // Try case-insensitive nested access
      let found = false
      for (const key of Object.keys(nested || {})) {
        if (key.toLowerCase() === part.toLowerCase()) {
          nested = nested[key]
          found = true
          break
        }
      }
      if (!found) {
        return undefined
      }
    }
  }
  return nested
}

// Type config factory matching CustomLayer
function getTypeConfig(t: (key: string) => string) {
  return {
    device: {
      label: t('customLayer.device'),
      icon: MapPin,
      color: 'text-primary-foreground',
      bgColor: 'bg-success',
    },
    metric: {
      label: t('customLayer.metric'),
      icon: Activity,
      color: 'text-primary-foreground',
      bgColor: 'bg-accent-purple',
    },
    command: {
      label: t('customLayer.command'),
      icon: Zap,
      color: 'text-primary-foreground',
      bgColor: 'bg-info',
    },
    text: {
      label: t('customLayer.text'),
      icon: Type,
      color: 'text-foreground',
      bgColor: 'bg-muted',
    },
    icon: {
      label: t('customLayer.iconLabel'),
      icon: Sparkles,
      color: 'text-primary-foreground',
      bgColor: 'bg-accent-orange',
    },
  } as const
}

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
  const { t } = useTranslation('dashboardComponents')
  const typeConfig = getTypeConfig(t)

  const [bindings, setBindings] = useState<LayerBinding[]>(initialBindings)
  const [selectedBinding, setSelectedBinding] = useState<string | null>(null)
  const [editingTextBinding, setEditingTextBinding] = useState<string | null>(null)
  const [editingIconBinding, setEditingIconBinding] = useState<string | null>(null)

  // Get devices from store for reactive updates
  const devices = useStore(state => state.devices)
  const deviceTelemetry = useStore(state => state.deviceTelemetry)

  // Reset state when dialog opens
  useEffect(() => {
    if (open) {
      setBindings(initialBindings)
      setSelectedBinding(null)
      setEditingTextBinding(null)
      setEditingIconBinding(null)
    }
  }, [open, initialBindings])

  // Convert bindings to layer items for preview
  const convertToLayerItems = useCallback((): LayerItem[] => {
    const getDeviceName = (deviceId: string) => {
      const device = findDevice(devices, deviceId)
      return device?.name || device?.device_id || deviceId
    }

    const getDeviceStatus = (deviceId: string): 'online' | 'offline' | 'error' | 'warning' | undefined => {
      const device = findDevice(devices, deviceId)
      if (!device) return undefined
      return device.online ? 'online' : 'offline'
    }

    const getDeviceMetricValue = (deviceId: string, metricId: string): string | number | undefined => {
      const device = findDevice(devices, deviceId)
      const cv = deviceTelemetry[deviceId] || device?.current_values
      if (!cv) return undefined
      const value = findMetricValue(cv, metricId || '')
      if (value !== undefined && value !== null) {
        return typeof value === 'number' ? value : String(value)
      }
      return undefined
    }

    return bindings.map((binding): LayerItem => {
      const position = binding.position === 'auto' || !binding.position
        ? { x: 50, y: 50 }
        : binding.position

      const ds = binding.dataSource
      const deviceId = ds.sourceId || (ds as any).deviceId || (ds.metricId ? ds.metricId.split(':')[0] : undefined)

      const item: LayerItem = {
        id: binding.id,
        type: binding.type || binding.icon || 'text',
        position,
        label: binding.name,
        color: binding.color,
        backgroundColor: binding.backgroundColor,
        fontSize: binding.fontSize,
        fontWeight: binding.fontWeight,
        opacity: binding.opacity,
        markerSize: binding.markerSize,
        visible: true,
        locked: false,
        draggable: true,
      }

      // Set type-specific fields
      if (binding.type === 'metric') {
        item.deviceId = deviceId
        item.metricId = ds.metricId || ds.property
        item.deviceName = getDeviceName(deviceId || '')
        item.metricName = ds.metricId || ds.property
        const metricValue = getDeviceMetricValue(deviceId || '', item.metricId || '')
        item.value = metricValue !== undefined ? metricValue : '-'
      } else if (binding.type === 'command') {
        item.command = ds.command
        item.deviceId = deviceId
        item.deviceName = getDeviceName(deviceId || '')
      } else if (binding.type === 'device') {
        item.deviceId = deviceId
        item.deviceName = getDeviceName(deviceId || '')
        item.status = getDeviceStatus(deviceId || '')
      } else if (binding.type === 'text') {
        item.value = (ds as any)?.text || ''
      } else if (binding.type === 'icon') {
        item.icon = (ds as any)?.icon || ''
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

  // Handle text content change
  const handleTextChange = useCallback((id: string, text: string) => {
    setBindings(prev => prev.map(b => {
      if (b.id === id && b.type === 'text') {
        return { ...b, dataSource: { ...(b.dataSource as any), text } }
      }
      return b
    }))
  }, [])

  // Handle icon content change
  const handleIconChange = useCallback((id: string, icon: string) => {
    setBindings(prev => prev.map(b => {
      if (b.id === id && b.type === 'icon') {
        return { ...b, dataSource: { ...(b.dataSource as any), icon } }
      }
      return b
    }))
  }, [])

  // Common icons for quick selection
  const commonIcons = ['⭐', '❤️', '🔥', '💡', '🏠', '🚗', '📱', '⚡', '💧', '🌡️', '📊', '📈', '🔔', '🎯', '✅', '❌', '⚠️', '🔴', '🟢', '🔵']

  // Handle opacity change
  const handleOpacityChange = useCallback((id: string, opacity: number) => {
    setBindings(prev => prev.map(b => b.id === id ? { ...b, opacity } : b))
  }, [])

  // Handle marker size change
  const handleMarkerSizeChange = useCallback((id: string, markerSize: 'xs' | 'sm' | 'md' | 'lg' | 'xl') => {
    setBindings(prev => prev.map(b => b.id === id ? { ...b, markerSize } : b))
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

  // Render binding item (shared between mobile and desktop)
  const renderBindingItem = (binding: LayerBinding) => {
    const config = typeConfig[binding.icon || binding.type]
    const Icon = config.icon
    const isSelected = selectedBinding === binding.id
    const isEditingText = editingTextBinding === binding.id
    const isEditingIcon = editingIconBinding === binding.id
    const ds = binding.dataSource as any

    return (
      <div
        key={binding.id}
        className={cn(
          'group flex flex-col gap-1 p-2 rounded-lg border transition-all cursor-pointer',
          isSelected
            ? 'border-primary bg-muted'
            : 'border-border hover:border-border hover:bg-muted-50'
        )}
        onClick={() => handleSelectBinding(binding.id)}
      >
        {/* Main row */}
        <div className="flex items-center gap-2">
          <GripVertical className="h-4 w-4 text-muted-foreground cursor-grab" />

          <div className={cn(
            'w-8 h-8 rounded-full flex items-center justify-center shrink-0',
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
              {binding.position === 'auto' && <span> • {t('customLayer.autoPosition')}</span>}
            </div>
          </div>

          <div className="flex items-center gap-1 opacity-0 group-hover:opacity-100 transition-opacity">
            {(binding.type === 'text' || binding.type === 'icon') && !isEditingText && !isEditingIcon && (
              <Button
                variant="ghost"
                size="icon"
                className="h-6 w-6"
                onClick={(e) => {
                  e.stopPropagation()
                  if (binding.type === 'text') setEditingTextBinding(binding.id)
                  if (binding.type === 'icon') setEditingIconBinding(binding.id)
                }}
                title={t('common.edit')}
              >
                <Edit3 className="h-4 w-4" />
              </Button>
            )}
            <Button
              variant="ghost"
              size="icon"
              className="h-6 w-6 text-error"
              onClick={(e) => {
                e.stopPropagation()
                handleRemoveBinding(binding.id)
              }}
              title={t('common.delete')}
            >
              <Trash2 className="h-4 w-4" />
            </Button>
          </div>
        </div>

        {/* Text editing panel */}
        {isEditingText && (
          <div className="space-y-2 pl-10 pr-2">
            <div className="flex items-center gap-2">
              <Label className="text-xs">{t('customLayer.textContent')}:</Label>
              <Input
                value={ds?.text || ''}
                onChange={(e) => handleTextChange(binding.id, e.target.value)}
                className="h-7 text-sm"
                onClick={(e) => e.stopPropagation()}
              />
            </div>
            <div className="flex items-center gap-2">
              <Label className="text-xs">{t('customLayer.name')}:</Label>
              <Input
                value={binding.name}
                onChange={(e) => {
                  setBindings(prev => prev.map(b => b.id === binding.id ? { ...b, name: e.target.value } : b))
                }}
                className="h-7 text-sm"
                onClick={(e) => e.stopPropagation()}
              />
              <Button
                size="sm"
                variant="ghost"
                className="h-6 px-2 text-xs"
                onClick={(e) => {
                  e.stopPropagation()
                  setEditingTextBinding(null)
                }}
              >
                <X className="h-4 w-4 mr-1" />
                {t('common.done')}
              </Button>
            </div>
          </div>
        )}

        {/* Icon editing panel */}
        {isEditingIcon && (
          <div className="space-y-2 pl-10 pr-2">
            <div className="flex items-center gap-2">
              <Label className="text-xs">{t('customLayer.icon')}:</Label>
              <Input
                value={ds?.icon || ''}
                onChange={(e) => handleIconChange(binding.id, e.target.value)}
                className="h-7 text-sm flex-1"
                placeholder={t('customLayer.iconPlaceholder')}
                onClick={(e) => e.stopPropagation()}
              />
            </div>
            <div className="flex flex-wrap gap-1">
              {commonIcons.map(icon => (
                <button
                  key={icon}
                  type="button"
                  className="w-8 h-8 flex items-center justify-center text-lg hover:bg-muted rounded border border-border"
                  onClick={(e) => {
                    e.stopPropagation()
                    handleIconChange(binding.id, icon)
                  }}
                >
                  {icon}
                </button>
              ))}
            </div>
            <div className="flex items-center gap-2">
              <Label className="text-xs w-16">{t('common.opacity')}:</Label>
              <div className="flex-1 flex items-center gap-2">
                <Slider
                  value={[binding.opacity ?? 100]}
                  min={0}
                  max={100}
                  step={5}
                  onValueChange={(values) => {
                    handleOpacityChange(binding.id, values[0])
                  }}
                  onClick={(e) => e.stopPropagation()}
                  className="flex-1"
                />
                <span className="text-xs text-muted-foreground w-8 text-right">
                  {binding.opacity ?? 100}%
                </span>
              </div>
            </div>
            <div className="flex items-center gap-2">
              <Label className="text-xs w-16">{t('common.size')}:</Label>
              <div className="flex-1 flex items-center gap-1">
                {(['xs', 'sm', 'md', 'lg', 'xl'] as const).map((size) => (
                  <button
                    key={size}
                    type="button"
                    className={cn(
                      'flex-1 h-7 rounded border text-xs font-medium transition-colors',
                      (binding.markerSize || 'md') === size
                        ? 'bg-primary text-primary-foreground border-primary'
                        : 'bg-muted hover:bg-accent border-border'
                    )}
                    onClick={(e) => {
                      e.stopPropagation()
                      handleMarkerSizeChange(binding.id, size)
                    }}
                  >
                    {size.toUpperCase()}
                  </button>
                ))}
              </div>
            </div>
            <div className="flex items-center gap-2">
              <Label className="text-xs">{t('customLayer.name')}:</Label>
              <Input
                value={binding.name}
                onChange={(e) => {
                  setBindings(prev => prev.map(b => b.id === binding.id ? { ...b, name: e.target.value } : b))
                }}
                className="h-7 text-sm"
                onClick={(e) => e.stopPropagation()}
              />
              <Button
                size="sm"
                variant="ghost"
                className="h-6 px-2 text-xs"
                onClick={(e) => {
                  e.stopPropagation()
                  setEditingIconBinding(null)
                }}
              >
                <X className="h-4 w-4 mr-1" />
                {t('common.done')}
              </Button>
            </div>
          </div>
        )}

        {/* Size and opacity controls for selected item */}
        {isSelected && !isEditingText && !isEditingIcon && (
          <div className="pl-10 pr-2 space-y-2">
            <div>
              <Label className="text-xs mb-1 block">{t('common.size')}:</Label>
              <div className="flex items-center gap-1">
                {(['xs', 'sm', 'md', 'lg', 'xl'] as const).map((size) => (
                  <button
                    key={size}
                    type="button"
                    className={cn(
                      'flex-1 h-7 rounded border text-xs font-medium transition-colors',
                      (binding.markerSize || 'md') === size
                        ? 'bg-primary text-primary-foreground border-primary'
                        : 'bg-muted hover:bg-accent border-border'
                    )}
                    onClick={(e) => {
                      e.stopPropagation()
                      handleMarkerSizeChange(binding.id, size)
                    }}
                  >
                    {size.toUpperCase()}
                  </button>
                ))}
              </div>
            </div>

            <div>
              <Label className="text-xs mb-1 block">{t('common.opacity')}:</Label>
              <div className="flex items-center gap-2">
                <Slider
                  value={[binding.opacity ?? 100]}
                  min={0}
                  max={100}
                  step={5}
                  onValueChange={(values) => {
                    handleOpacityChange(binding.id, values[0])
                  }}
                  onClick={(e) => e.stopPropagation()}
                  className="flex-1"
                />
                <span className="text-xs text-muted-foreground w-10 text-right">
                  {binding.opacity ?? 100}%
                </span>
              </div>
            </div>
          </div>
        )}
      </div>
    )
  }

  return (
    <UnifiedFormDialog
      open={open}
      onOpenChange={onOpenChange}
      title={t('customLayer.editorTitle')}
      icon={<Layers className="h-5 w-5 text-primary" />}
      width="3xl"
      className="sm:h-[90vh] z-[110]"
      contentClassName="p-0 flex flex-col overflow-hidden"
      preventCloseOnSubmit={false}
      footer={
        <>
          <Button variant="outline" onClick={() => onOpenChange(false)}>
            {t('common.cancel')}
          </Button>
          <Button onClick={handleSave}>
            <Check className="h-4 w-4 mr-2" />
            {t('common.save')}
          </Button>
        </>
      }
    >
      <div className="flex-1 flex overflow-hidden min-h-0">
        {/* Left Panel - Bindings List */}
        <div className="w-72 bg-muted-20 flex flex-col shrink-0">
          <div className="px-4 py-2 border-b bg-muted-30 shrink-0">
            <div className="text-xs font-medium text-muted-foreground uppercase tracking-wide">
              {t('customLayer.boundItems')} ({bindings.length})
            </div>
          </div>

          <div className="flex-1 overflow-y-auto p-2 space-y-1 min-h-0">
            {bindings.length === 0 ? (
              <div className="text-center py-8 text-muted-foreground">
                <Layers className="h-8 w-8 mx-auto mb-2 opacity-50" />
                <p className="text-sm">{t('customLayer.noItems')}</p>
                <p className="text-xs mt-1">{t('customLayer.addDataSourceHint')}</p>
              </div>
            ) : (
              bindings.map(renderBindingItem)
            )}
          </div>
        </div>

        {/* Right Panel - Layer Preview */}
        <div className="flex-1 relative bg-muted-30 min-w-0">
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
              {t('customLayer.clickToSetPosition')}
            </div>
          )}
        </div>
      </div>
    </UnifiedFormDialog>
  )
}
