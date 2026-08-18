/**
 * Map Editor Dialog
 *
 * Map marker position editor with:
 * - Left panel: List of bound items (devices/metrics/commands)
 * - Right panel: Large interactive map preview
 * - Click to set marker position
 * - Drag to reposition markers
 */

import { useState, useCallback, useEffect } from 'react'
import { useTranslation } from 'react-i18next'
import { Button } from '@/components/ui/button'
import { cn } from '@/lib/utils'
import {
  Trash2,
  GripVertical,
  Check,
  Activity,
  Zap,
  Map as MapIcon,
  MapPin,
  Monitor,
} from 'lucide-react'
import { MapDisplay, type MapMarker } from './MapDisplay'
import type { DataSource } from '@/types/dashboard'
import { useStore } from '@/store'
import { UnifiedFormDialog } from '@/components/dialog/UnifiedFormDialog'

export type MapBindingType = 'device' | 'metric' | 'command' | 'marker'

export interface MapBinding {
  id: string
  type: MapBindingType
  name: string
  dataSource: DataSource
  position?: { lat: number; lng: number } | 'auto'
  icon?: MapBindingType
}

interface MapEditorDialogProps {
  open: boolean
  onOpenChange: (open: boolean) => void
  bindings: MapBinding[]
  center: { lat: number; lng: number }
  zoom: number
  tileLayer: string
  onSave: (bindings: MapBinding[]) => void
}

// Type config factory
function getTypeConfig(t: (key: string) => string) {
  return {
    device: {
      label: t('mapDisplay.device'),
      icon: MapPin,
      color: 'text-success',
      bgColor: 'bg-success',
    },
    metric: {
      label: t('mapDisplay.metric'),
      icon: Activity,
      color: 'text-accent-purple',
      bgColor: 'bg-accent-purple',
    },
    command: {
      label: t('mapDisplay.command'),
      icon: Zap,
      color: 'text-info',
      bgColor: 'bg-info',
    },
    marker: {
      label: t('mapDisplay.marker'),
      icon: Monitor,
      color: 'text-accent-orange',
      bgColor: 'bg-accent-orange',
    },
  } as const
}

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

export function MapEditorDialog({
  open,
  onOpenChange,
  bindings: initialBindings,
  center,
  zoom,
  tileLayer,
  onSave,
}: MapEditorDialogProps) {
  const { t } = useTranslation('dashboardComponents')
  const typeConfig = getTypeConfig(t)

  const [bindings, setBindings] = useState<MapBinding[]>(initialBindings)
  const [selectedBinding, setSelectedBinding] = useState<string | null>(null)

  // Get devices from store for reactive updates
  const devices = useStore(state => state.devices)
  const deviceTelemetry = useStore(state => state.deviceTelemetry)

  // Reset state when dialog opens
  useEffect(() => {
    if (open) {
      setBindings(initialBindings)
      setSelectedBinding(null)
    }
  }, [open, initialBindings])

  // Convert bindings to map markers for preview
  const convertToMarkers = useCallback((): MapMarker[] => {
    const storeDevices = devices
    const getDeviceName = (deviceId: string) => {
      const device = storeDevices.find(d => d.id === deviceId || d.device_id === deviceId)
      return device?.name || device?.device_id || deviceId
    }

    const getDeviceStatus = (deviceId: string): 'online' | 'offline' | 'error' | 'warning' | undefined => {
      const device = storeDevices.find(d => d.id === deviceId || d.device_id === deviceId)
      if (!device) return undefined
      return device.online ? 'online' : 'offline'
    }

    const markers = bindings.map((binding): MapMarker => {
      const itemConfig = typeConfig[binding.icon || binding.type]
      const ds = binding.dataSource
      const sourceId = ds.sourceId || (ds as any).deviceId || (ds.metricId ? ds.metricId.split(':')[0] : undefined)
      const lat = binding.position === 'auto' || !binding.position
        ? center.lat
        : binding.position.lat
      const lng = binding.position === 'auto' || !binding.position
        ? center.lng
        : binding.position.lng

      const device = sourceId ? storeDevices.find(d => d.id === sourceId || d.device_id === sourceId) : undefined

      let metricValue: string | undefined = undefined
      if (binding.type === 'metric' && sourceId) {
        const metricKey = ds.metricId || ds.property
        const cv = deviceTelemetry[sourceId] || device?.current_values
        if (cv && metricKey) {
          const rawValue = findMetricValue(cv, metricKey)
          if (rawValue !== undefined && rawValue !== null) {
            metricValue = typeof rawValue === 'number'
              ? rawValue.toFixed(1)
              : String(rawValue)
          }
        }
      }

      return {
        id: binding.id,
        latitude: lat,
        longitude: lng,
        label: binding.name,
        markerType: binding.icon || binding.type,
        status: binding.type === 'device' ? getDeviceStatus(sourceId || '') : undefined,
        metricValue: binding.type === 'metric' ? (metricValue || '-') : undefined,
        deviceId: sourceId,
        sourceId,
        deviceName: sourceId ? getDeviceName(sourceId) : undefined,
        metricName: ds.metricId || ds.property,
        command: binding.type === 'command' ? ds.command : undefined,
        commandName: binding.type === 'command' ? ds.command : undefined,
      }
    })
    return markers
  }, [bindings, center, devices])

  const mapMarkers = convertToMarkers()

  // Handle removing a binding
  const handleRemoveBinding = useCallback((id: string) => {
    setBindings(bindings.filter(b => b.id !== id))
    if (selectedBinding === id) {
      setSelectedBinding(null)
    }
  }, [bindings, selectedBinding])

  // Handle updating binding position from map click
  const handleMapClick = useCallback((lat: number, lng: number) => {
    if (selectedBinding) {
      setBindings(prev => prev.map(b =>
        b.id === selectedBinding
          ? { ...b, position: { lat, lng } }
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

  // Render binding item
  const renderBindingItem = (binding: MapBinding) => {
    const config = typeConfig[binding.icon || binding.type]
    const Icon = config.icon
    const isSelected = selectedBinding === binding.id

    return (
      <div
        key={binding.id}
        className={cn(
          'group flex items-center gap-2 p-2 rounded-lg border transition-all cursor-pointer',
          isSelected
            ? 'border-primary bg-muted'
            : 'border-border hover:border-border hover:bg-muted-50'
        )}
        onClick={() => handleSelectBinding(binding.id)}
      >
        <GripVertical className="h-4 w-4 text-muted-foreground cursor-grab" />

        <div className={cn(
          'w-8 h-8 rounded-full flex items-center justify-center',
          config.bgColor === 'bg-info' && 'bg-info text-primary-foreground',
          config.bgColor === 'bg-success' && 'bg-success text-primary-foreground',
          config.bgColor === 'bg-accent-orange' && 'bg-accent-orange text-primary-foreground',
          config.bgColor === 'bg-accent-purple' && 'bg-accent-purple text-primary-foreground',
        )}>
          <Icon className="h-4 w-4 text-primary-foreground" />
        </div>

        <div className="flex-1 min-w-0">
          <div className="text-sm font-medium truncate">{binding.name}</div>
          <div className="text-xs text-muted-foreground truncate">
            {config.label}
            {binding.position && binding.position !== 'auto' && (
              <span> • ({binding.position.lat.toFixed(2)}, {binding.position.lng.toFixed(2)})</span>
            )}
            {binding.position === 'auto' && <span> • {t('mapDisplay.autoPosition')}</span>}
          </div>
        </div>

        <div className="flex items-center gap-1 opacity-0 group-hover:opacity-100 transition-opacity">
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
    )
  }

  return (
    <UnifiedFormDialog
      open={open}
      onOpenChange={onOpenChange}
      title={t('mapDisplay.editorTitle')}
      icon={<MapIcon className="h-5 w-5 text-primary" />}
      width="3xl"
      className="sm:h-[90vh] z-[110]"
      contentClassName="p-0 flex flex-col overflow-hidden"
      preventCloseOnSubmit={false}
      footer={
        <>
          <Button variant="outline" onClick={() => onSave(undefined as any)}>
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
              {t('mapDisplay.boundItems')} ({bindings.length})
            </div>
          </div>

          <div className="flex-1 overflow-y-auto p-2 space-y-1">
            {bindings.length === 0 ? (
              <div className="text-center py-8 text-muted-foreground">
                <MapIcon className="h-8 w-8 mx-auto mb-2 opacity-50" />
                <p className="text-sm">{t('mapDisplay.noMarkers')}</p>
                <p className="text-xs mt-1">{t('mapDisplay.addDataSourceHint')}</p>
              </div>
            ) : (
              bindings.map(renderBindingItem)
            )}
          </div>
        </div>

        {/* Right Panel - Map Preview */}
        <div className="flex-1 relative bg-muted-30 min-w-0">
          <div className="absolute inset-0">
            <MapDisplay
              center={center}
              zoom={zoom}
              tileLayer={tileLayer}
              markers={mapMarkers}
              showControls={true}
              showFullscreen={false}
              interactive={true}
              onMapClick={handleMapClick}
              className="w-full h-full"
            />
          </div>
        </div>
      </div>
    </UnifiedFormDialog>
  )
}
