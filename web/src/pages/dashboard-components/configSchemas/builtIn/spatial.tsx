import React from 'react'
import { cn } from '@/lib/utils'
import { compressImageFile } from '@/lib/imageUtils'
import { chartColorsHex } from '@/design-system/tokens/color'
import { Field } from '@/components/ui/field'
import { Label } from '@/components/ui/label'
import { Input } from '@/components/ui/input'
import { Textarea } from '@/components/ui/textarea'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import { Checkbox } from '@/components/ui/checkbox'
import { ColorPicker } from '@/components/ui/color-picker'
import { IconPicker } from '@/components/ui/icon-picker'
import { EntityIconPicker } from '@/components/ui/entity-icon-picker'
import { DataMappingConfig } from '@/components/dashboard/config/UIConfigSections'
import { LEDStateRulesConfig } from '@/components/dashboard/config/LEDStateRulesConfig'
import type { StateRule } from '@/components/dashboard/generic/LEDIndicator'
import type { SingleValueMappingConfig, TimeSeriesMappingConfig, CategoricalMappingConfig } from '@/lib/dataMapping'
import { DualModeSourceField } from '@/components/dashboard/config'
import type { ComponentConfigSchema } from '@/components/dashboard/config/ComponentConfigBuilder'
import { SelectField } from '../../ConfigFieldComponents'
import type { SchemaContext, Updaters } from '../types'
import { useStore } from '@/store'
import { getSourceId } from '@/types/dashboard'
import type { DataSource } from '@/types/dashboard'
import { Button } from '@/components/ui/button'
import { DeviceBindingConfig } from '@/components/dashboard/config/DeviceBindingConfig'
import type { MapBinding, MapBindingType } from '@/components/dashboard/generic/MapEditorDialog'
import type { LayerBinding, LayerBindingType } from '@/components/dashboard/generic/CustomLayer'
import { BindingDataSourceSelector } from '../../BindingDataSourceSelector'
import {
  MapPin,
  Map as MapIcon,
  Layers,
  Activity,
  Zap,
  Monitor,
  Type,
  Sparkles,
  Upload,
} from 'lucide-react'

export function getMapDisplaySchema(config: any, ctx: SchemaContext, u: Updaters): ComponentConfigSchema {
  const { t, setCenterPickerOpen, setMapEditorBindings, setMapEditorOpen, currentDashboard, selectedComponent } = ctx
  const { updateConfig, updateDataSource, updateDataMapping, updateNestedConfig } = u
  return {
          dataSourceSections: [],
          styleSections: [
            {
              type: 'custom' as const,
              render: () => (
                <div className="space-y-3">
                  <div className="flex items-end gap-2">
                    <div className="grid grid-cols-[minmax(0,1fr)_minmax(0,1fr)] gap-3 flex-1">
                      <Field>
                        <Label>{t('visualDashboard.latitude')}</Label>
                        <Input
                          type="number"
                          step="0.0001"
                          value={(config.center as { lat: number } | undefined)?.lat ?? 39.9042}
                          onChange={(e) => updateConfig('center')({ ...(config.center as { lat: number; lng: number } | undefined) || { lat: 39.9042, lng: 116.4074 }, lat: parseFloat(e.target.value) })}
                          placeholder={t('mapDisplay.defaultLatitude', '39.9042')}
                          className="h-9"
                        />
                      </Field>
                      <Field>
                        <Label>{t('visualDashboard.longitude')}</Label>
                        <Input
                          type="number"
                          step="0.0001"
                          value={(config.center as { lng: number } | undefined)?.lng ?? 116.4074}
                          onChange={(e) => updateConfig('center')({ ...(config.center as { lat: number; lng: number } | undefined) || { lat: 39.9042, lng: 116.4074 }, lng: parseFloat(e.target.value) })}
                          placeholder={t('mapDisplay.defaultLongitude', '116.4074')}
                          className="h-9"
                        />
                      </Field>
                    </div>
                    <Button
                      variant="outline"
                      size="sm"
                      onClick={() => setCenterPickerOpen(true)}
                      className="h-9 px-3 shrink-0"
                      title={t('mapDisplay.visualSelectCenter', '可视化选择中心点')}
                    >
                      <MapPin className="h-4 w-4" />
                    </Button>
                  </div>

                  <div className="grid grid-cols-[minmax(0,1fr)_minmax(0,1fr)_minmax(0,1fr)] gap-3">
                    <Field>
                      <Label>{t('visualDashboard.zoomLevel')}</Label>
                      <Input
                        type="number"
                        min={config.minZoom ?? 2}
                        max={config.maxZoom ?? 18}
                        value={config.zoom ?? 10}
                        onChange={(e) => updateConfig('zoom')(parseFloat(e.target.value))}
                        className="h-9"
                      />
                    </Field>
                    <Field>
                      <Label>{t('visualDashboard.maxZoom')}</Label>
                      <Input
                        type="number"
                        min={1}
                        max={10}
                        value={config.minZoom ?? 2}
                        onChange={(e) => updateConfig('minZoom')(parseFloat(e.target.value))}
                        className="h-9"
                      />
                    </Field>
                    <Field>
                      <Label>{t('visualDashboard.maxZoom')}</Label>
                      <Input
                        type="number"
                        min={10}
                        max={20}
                        value={config.maxZoom ?? 18}
                        onChange={(e) => updateConfig('maxZoom')(parseFloat(e.target.value))}
                        className="h-9"
                      />
                    </Field>
                  </div>

                  <SelectField
                    label={t('visualDashboard.mapLayer')}
                    value={config.tileLayer || 'osm'}
                    onChange={updateConfig('tileLayer')}
                    options={[
                      { value: 'osm', label: 'OpenStreetMap' },
                      { value: 'satellite', label: t('visualDashboard.satellite') },
                      { value: 'dark', label: t('visualDashboard.darkMode') },
                      { value: 'terrain', label: t('visualDashboard.terrain') },
                    ]}
                  />

                  <Field>
                    <Label>{t('visualDashboard.markerColor')}</Label>
                    <Input
                      type="color"
                      value={config.markerColor || chartColorsHex[0]}
                      onChange={(e) => updateConfig('markerColor')(e.target.value)}
                      className="h-9 w-full"
                    />
                  </Field>

                  <div className="grid grid-cols-[minmax(0,1fr)_minmax(0,1fr)] gap-3">
                    <Field>
                      <Label>{t('visualDashboard.showControlBar')}</Label>
                      <Select
                        value={String(config.showControls ?? true)}
                        onValueChange={(value) => updateConfig('showControls')(value === 'true')}
                      >
                        <SelectTrigger className="w-full h-9">
                          <SelectValue />
                        </SelectTrigger>
                        <SelectContent>
                          <SelectItem value="true">{t('visualDashboard.showCard')}</SelectItem>
                          <SelectItem value="false">{t('visualDashboard.hide')}</SelectItem>
                        </SelectContent>
                      </Select>
                    </Field>
                    <Field>
                      <Label>{t('visualDashboard.showLayerControl')}</Label>
                      <Select
                        value={String(config.showLayers ?? true)}
                        onValueChange={(value) => updateConfig('showLayers')(value === 'true')}
                      >
                        <SelectTrigger className="w-full h-9">
                          <SelectValue />
                        </SelectTrigger>
                        <SelectContent>
                          <SelectItem value="true">{t('visualDashboard.showCard')}</SelectItem>
                          <SelectItem value="false">{t('visualDashboard.hide')}</SelectItem>
                        </SelectContent>
                      </Select>
                    </Field>
                  </div>

                  <div className="grid grid-cols-[minmax(0,1fr)_minmax(0,1fr)] gap-3">
                    <Field>
                      <Label>{t('visualDashboard.interactive')}</Label>
                      <Select
                        value={String(config.interactive ?? true)}
                        onValueChange={(value) => updateConfig('interactive')(value === 'true')}
                      >
                        <SelectTrigger className="w-full h-9">
                          <SelectValue />
                        </SelectTrigger>
                        <SelectContent>
                          <SelectItem value="true">{t('visualDashboard.yes')}</SelectItem>
                          <SelectItem value="false">{t('visualDashboard.no')}</SelectItem>
                        </SelectContent>
                      </Select>
                    </Field>
                    <Field>
                      <Label>{t('visualDashboard.fullscreenButton')}</Label>
                      <Select
                        value={String(config.showFullscreen ?? true)}
                        onValueChange={(value) => updateConfig('showFullscreen')(value === 'true')}
                      >
                        <SelectTrigger className="w-full h-9">
                          <SelectValue />
                        </SelectTrigger>
                        <SelectContent>
                          <SelectItem value="true">{t('visualDashboard.showCard')}</SelectItem>
                          <SelectItem value="false">{t('visualDashboard.hide')}</SelectItem>
                        </SelectContent>
                      </Select>
                    </Field>
                  </div>
                </div>
              ),
            },
          ],
          displaySections: [
            {
              type: 'custom' as const,
              render: () => (
                <div className="space-y-4">
                  {/* Data source selection — merged from Data Source tab */}
                  <BindingDataSourceSelector
                    dataSource={config.dataSource}
                    onConfirm={(newSource) => {
                      updateDataSource(newSource)
                      if (Array.isArray(newSource) && newSource.length > 0) {
                        const newBindings: MapBinding[] = newSource.map((ds, index) => {
                          let bindingType: MapBindingType = 'device'
                          if (ds.type === 'metric' || ds.type === 'telemetry') bindingType = 'metric'
                          else if (ds.type === 'command') bindingType = 'command'

                          const existingBinding = (config.bindings as MapBinding[])?.find(b => {
                            if (!b.dataSource) return false
                            const bDs = b.dataSource as any
                            if (bindingType === 'metric' || ds.type === 'telemetry') {
                              return (getSourceId(bDs) === getSourceId(ds)) && (
                                bDs.metricId === ds.metricId ||
                                bDs.property === ds.metricId ||
                                bDs.property === ds.property
                              )
                            }
                            if (bindingType === 'command') {
                              return (getSourceId(bDs) === getSourceId(ds)) && (bDs.command === ds.command)
                            }
                            return getSourceId(bDs) === getSourceId(ds) && !ds.metricId && !ds.property && !ds.command
                          })

                          const generateBindingId = () => {
                            if (ds.type === 'metric' || ds.type === 'telemetry') {
                              return `${bindingType}-${getSourceId(ds)}-${ds.metricId || ds.property || index}`
                            } else if (ds.type === 'command') {
                              return `${bindingType}-${getSourceId(ds)}-${ds.command}`
                            } else {
                              return `${bindingType}-${getSourceId(ds)}-${index}`
                            }
                          }

                          const baseBinding = existingBinding || {
                            id: generateBindingId(),
                            position: { lat: 39.9042, lng: 116.4074 },
                          }

                          return {
                            ...baseBinding,
                            id: existingBinding?.id || generateBindingId(),
                            type: bindingType,
                            icon: bindingType,
                            name: (ds.type === 'metric' || ds.type === 'telemetry')
                              ? (ds.metricId || ds.property || t('visualDashboard.metricIndex', { index: index + 1 }))
                              : ds.type === 'command'
                                ? `${getSourceId(ds) || ''} → ${ds.command || ''}`
                                : (getSourceId(ds) || t('visualDashboard.deviceIndex', { index: index + 1 })),
                            dataSource: ds,
                            position: existingBinding?.position || baseBinding.position,
                          }
                        })
                        updateConfig('bindings')(newBindings)
                      }
                    }}
                    allowedTypes={['device', 'metric', 'command', 'extension']}
                    maxSources={50}
                    title={t('visualDashboard.markerBinding')}
                  />

                  <div className="flex items-center justify-between">
                    <div>
                      <h3 className="text-sm font-medium">{t('visualDashboard.markerBinding')}</h3>
                      <p className="text-xs text-muted-foreground mt-1">
                        {t('visualDashboard.manageMapMarkers')}
                      </p>
                    </div>
                    <Button
                      type="button"
                      variant="default"
                      size="sm"
                      onClick={() => {
                        // Get the latest bindings from the store, not just local state
                        const latestDashboard = useStore.getState().currentDashboard
                        const latestComponent = latestDashboard?.components.find(c => c.id === selectedComponent?.id)
                        let latestBindings = (latestComponent as any)?.config?.bindings as MapBinding[] || []

                        // Fix duplicate IDs - regenerate IDs for bindings with duplicate IDs
                        const idCount = new Map<string, number>()
                        latestBindings = latestBindings.map((binding, index) => {
                          const ds = binding.dataSource as any
                          const currentId = binding.id

                          // Check if this ID is duplicated
                          idCount.set(currentId, (idCount.get(currentId) || 0) + 1)

                          // If ID will be duplicated or uses old format, regenerate it
                          if (idCount.get(currentId)! > 1 || binding.type === 'device' && ds?.metricId) {
                            // Generate unique ID based on type and data
                            let newId: string
                            if (binding.type === 'metric' || ds?.type === 'telemetry') {
                              newId = `metric-${getSourceId(ds)}-${ds?.metricId || ds?.property || index}`
                            } else if (binding.type === 'command') {
                              newId = `command-${getSourceId(ds)}-${ds?.command}`
                            } else {
                              newId = `device-${getSourceId(ds)}-${index}`
                            }
                                                        return { ...binding, id: newId }
                          }

                          return binding
                        })

                        setMapEditorBindings(latestBindings)
                        setMapEditorOpen(true)
                      }}
                    >
                      <MapIcon className="h-4 w-4 mr-1" />
                      {t('visualDashboard.openMapEditor')}
                    </Button>
                  </div>

                  {/* Bindings List - Grouped by Type */}
                  <div className="border rounded-lg overflow-hidden">
                    {(() => {
                      // Get the latest bindings from the store for display
                      const latestDashboard = useStore.getState().currentDashboard
                      const latestComponent = latestDashboard?.components.find(c => c.id === selectedComponent?.id)
                      let displayBindings = (latestComponent as any)?.config?.bindings as MapBinding[] || []

                      // Fix duplicate IDs for display and interaction
                      const idCount = new Map<string, number>()
                      displayBindings = displayBindings.map((binding, index) => {
                        const ds = binding.dataSource as any
                        const currentId = binding.id
                        idCount.set(currentId, (idCount.get(currentId) || 0) + 1)

                        // If ID is duplicated or binding type is wrong (e.g., telemetry marked as device)
                        if (idCount.get(currentId)! > 1 || (binding.type === 'device' && ds?.type === 'telemetry')) {
                          let newId: string
                          let newType = binding.type

                          // Fix type for telemetry bindings
                          if (ds?.type === 'telemetry' || ds?.type === 'metric') {
                            newType = 'metric'
                          }

                          if (newType === 'metric' || ds?.type === 'telemetry') {
                            newId = `metric-${getSourceId(ds)}-${ds?.metricId || ds?.property || index}`
                          } else if (newType === 'command') {
                            newId = `command-${getSourceId(ds)}-${ds?.command}`
                          } else {
                            newId = `device-${getSourceId(ds)}-${index}`
                          }
                                                    return { ...binding, id: newId, type: newType as any, icon: newType as any }
                        }
                        return binding
                      })

                      // Group by type
                      const groupedBindings = {
                        device: displayBindings.filter(b => b.type === 'device'),
                        metric: displayBindings.filter(b => b.type === 'metric'),
                        command: displayBindings.filter(b => b.type === 'command'),
                        marker: displayBindings.filter(b => b.type === 'marker'),
                      }

                      const TYPE_CONFIG = {
                        device: {
                          label: t('mapDisplay.device'),
                          color: 'bg-success',
                          textColor: 'text-success',
                          bgColor: 'bg-success-light',
                          borderColor: 'border-success-light dark:border-success-light',
                          icon: MapPin,
                          description: t('mapDisplay.deviceDesc')
                        },
                        metric: {
                          label: t('mapDisplay.metric'),
                          color: 'bg-accent-purple',
                          textColor: 'text-accent-purple',
                          bgColor: 'bg-accent-purple-light',
                          borderColor: 'border-accent-purple-light',
                          icon: Activity,
                          description: t('mapDisplay.metricDesc')
                        },
                        command: {
                          label: t('mapDisplay.command'),
                          color: 'bg-info',
                          textColor: 'text-info',
                          bgColor: 'bg-info-light',
                          borderColor: 'border-info',
                          icon: Zap,
                          description: t('mapDisplay.commandDesc')
                        },
                        marker: {
                          label: t('mapDisplay.marker'),
                          color: 'bg-accent-orange',
                          textColor: 'text-accent-orange',
                          bgColor: 'bg-accent-orange-light',
                          borderColor: 'border-accent-orange-light',
                          icon: Monitor,
                          description: t('mapDisplay.markerDesc')
                        },
                      } as const

                      if (displayBindings.length === 0) {
                        return (
                          <div className="p-6 text-center text-muted-foreground">
                            <MapIcon className="h-8 w-8 mx-auto mb-2 opacity-50" />
                            <p className="text-sm">{t('visualDashboard.noMarkers')}</p>
                            <p className="text-xs mt-1">{t('visualDashboard.addMarkerHint')}</p>
                          </div>
                        )
                      }

                      return (Object.keys(groupedBindings) as Array<keyof typeof groupedBindings>).map(type => {
                        const typeBindings = groupedBindings[type]
                        if (typeBindings.length === 0) return null

                        const config = TYPE_CONFIG[type]
                        const Icon = config.icon

                        return (
                          <div key={type} className="border-b last:border-b-0">
                            {/* Type Header */}
                            <div className={`px-3 py-2 ${config.bgColor} border-b ${config.borderColor} flex items-center justify-between`}>
                              <div className="flex items-center gap-2">
                                <div className={`w-5 h-5 rounded-full ${config.color} flex items-center justify-center`}>
                                  <Icon className="h-4 w-4 text-primary-foreground" />
                                </div>
                                <span className="text-sm font-medium">{config.label}</span>
                                <span className="text-xs text-muted-foreground">({typeBindings.length})</span>
                              </div>
                              <span className="text-xs text-muted-foreground">{config.description}</span>
                            </div>

                            {/* Bindings of this type */}
                            <div className="divide-y">
                              {typeBindings.map((binding) => {
                                const positionText = binding.position && binding.position !== 'auto'
                                  ? `(${binding.position.lat.toFixed(4)}, ${binding.position.lng.toFixed(4)})`
                                  : t('visualDashboard.autoLocation')

                                // Get device/metric info from dataSource
                                const deviceId = getSourceId((binding.dataSource as DataSource))
                                const metricId = (binding.dataSource as any)?.metricId
                                const command = (binding.dataSource as any)?.command

                                return (
                                  <div
                                    key={binding.id}
                                    className="flex items-center gap-3 p-3"
                                  >
                                    <div className={`w-8 h-8 rounded-full flex items-center justify-center ${config.color}/20 ${config.textColor}`}>
                                      <Icon className="h-4 w-4" />
                                    </div>
                                    <div className="flex-1 min-w-0">
                                      <div className="text-sm font-medium truncate">{binding.name}</div>
                                      <div className="flex items-center gap-2 text-xs text-muted-foreground">
                                        <span>{positionText}</span>
                                        {deviceId && <span>• {deviceId.slice(0, 8)}...</span>}
                                        {metricId && <span>• {metricId}</span>}
                                        {command && <span>• {command}</span>}
                                      </div>
                                    </div>
                                  </div>
                                )
                              })}
                            </div>
                          </div>
                        )
                      })
                    })()}
                  </div>

                  {/* Legend */}
                  <div className="flex items-center gap-4 text-xs text-muted-foreground">
                    <div className="flex items-center gap-1">
                      <div className="w-4 h-4 rounded-full bg-info"></div>
                      <span>{t('mapDisplay.device')}</span>
                    </div>
                    <div className="flex items-center gap-1">
                      <div className="w-4 h-4 rounded-full bg-success"></div>
                      <span>{t('mapDisplay.metric')}</span>
                    </div>
                    <div className="flex items-center gap-1">
                      <div className="w-4 h-4 rounded-full bg-accent-orange"></div>
                      <span>{t('mapDisplay.command')}</span>
                    </div>
                    <div className="flex items-center gap-1">
                      <div className="w-4 h-4 rounded-full bg-accent-purple"></div>
                      <span>{t('mapDisplay.marker')}</span>
                    </div>
                  </div>
                </div>
              ),
            },
          ],
        }
}

export function getCustomLayerSchema(config: any, ctx: SchemaContext, u: Updaters): ComponentConfigSchema {
  const { t, setLayerEditorBindings, setLayerEditorOpen, currentDashboard, selectedComponent } = ctx
  const { updateConfig, updateDataSource, updateDataMapping, updateNestedConfig } = u
  return {
          dataSourceSections: [],
          styleSections: [
            {
              type: 'custom' as const,
              render: () => (
                <div className="space-y-3">
                  <SelectField
                    label={t('visualDashboard.backgroundType')}
                    value={config.backgroundType || 'grid'}
                    onChange={updateConfig('backgroundType')}
                    options={[
                      { value: 'grid', label: t('visualDashboard.backgroundTypeGrid') },
                      { value: 'color', label: t('visualDashboard.backgroundTypeColor') },
                      { value: 'image', label: t('visualDashboard.backgroundTypeImage') },
                      { value: 'transparent', label: t('visualDashboard.backgroundTypeTransparent') },
                    ]}
                  />

                  {config.backgroundType === 'color' && (
                    <Field>
                      <Label>{t('visualDashboard.backgroundColor')}</Label>
                      <Input
                        type="color"
                        value={config.backgroundColor || '#e5e5e5'}
                        onChange={(e) => updateConfig('backgroundColor')(e.target.value)}
                        className="h-9 w-full"
                      />
                    </Field>
                  )}

                  {config.backgroundType === 'image' && (
                    <>
                      <Field>
                        <Label>{t('visualDashboard.backgroundImageUrl')}</Label>
                        <div className="flex gap-2">
                          <Input
                            value={config.backgroundImage || ''}
                            onChange={(e) => updateConfig('backgroundImage')(e.target.value)}
                            placeholder={t('placeholders.urlExample')}
                            className="h-9 flex-1"
                          />
                          <Button
                            type="button"
                            variant="outline"
                            size="sm"
                            onClick={() => {
                              const input = document.createElement('input')
                              input.type = 'file'
                              input.accept = 'image/*'
                              input.onchange = async (e) => {
                                const file = (e.target as HTMLInputElement).files?.[0]
                                if (file) {
                                  try {
                                    const compressed = await compressImageFile(file)
                                    updateConfig('backgroundImage')(compressed)
                                  } catch { /* ignore */ }
                                }
                              }
                              input.click()
                            }}
                            className="h-9 px-3 shrink-0"
                          >
                            <Upload className="h-4 w-4 mr-1.5" />
                            {t('visualDashboard.upload')}
                          </Button>
                        </div>
                      </Field>
                      {config.backgroundImage && (
                        <div className="flex items-center gap-2">
                          <div className="w-12 h-12 rounded border overflow-hidden bg-muted-30">
                            <img
                              src={config.backgroundImage}
                              alt="Preview"
                              className="w-full h-full object-cover"
                            />
                          </div>
                          <Button
                            type="button"
                            variant="ghost"
                            size="sm"
                            onClick={() => updateConfig('backgroundImage')('')}
                            className="h-8 text-error hover:text-error"
                          >
                            {t('visualDashboard.clear')}
                          </Button>
                        </div>
                      )}
                    </>
                  )}

                  {config.backgroundType === 'grid' && (
                    <Field>
                      <Label>{t('visualDashboard.gridSize')}</Label>
                      <Input
                        type="number"
                        min={10}
                        max={50}
                        value={config.gridSize ?? 20}
                        onChange={(e) => updateConfig('gridSize')(Number(e.target.value))}
                        className="h-9"
                      />
                    </Field>
                  )}

                  <div className="grid grid-cols-[minmax(0,1fr)_minmax(0,1fr)] gap-3">
                    <Field>
                      <Label>{t('visualDashboard.showControlBar')}</Label>
                      <Select
                        value={String(config.showControls ?? true)}
                        onValueChange={(value) => updateConfig('showControls')(value === 'true')}
                      >
                        <SelectTrigger className="w-full h-9">
                          <SelectValue />
                        </SelectTrigger>
                        <SelectContent>
                          <SelectItem value="true">{t('visualDashboard.showCard')}</SelectItem>
                          <SelectItem value="false">{t('visualDashboard.hide')}</SelectItem>
                        </SelectContent>
                      </Select>
                    </Field>
                    <Field>
                      <Label>{t('visualDashboard.showFullscreenButton')}</Label>
                      <Select
                        value={String(config.showFullscreen ?? true)}
                        onValueChange={(value) => updateConfig('showFullscreen')(value === 'true')}
                      >
                        <SelectTrigger className="w-full h-9">
                          <SelectValue />
                        </SelectTrigger>
                        <SelectContent>
                          <SelectItem value="true">{t('visualDashboard.showCard')}</SelectItem>
                          <SelectItem value="false">{t('visualDashboard.hide')}</SelectItem>
                        </SelectContent>
                      </Select>
                    </Field>
                  </div>
                </div>
              ),
            },
          ],
          displaySections: [
            {
              type: 'custom' as const,
              render: () => (
                <div className="space-y-4">
                  {/* Data source selection — merged from Data Source tab */}
                  <BindingDataSourceSelector
                    dataSource={config.bindings as any}
                    onConfirm={(newDataSources) => {
                      const sourcesArray = newDataSources
                        ? Array.isArray(newDataSources)
                          ? newDataSources
                          : [newDataSources]
                        : []

                      const newBindings = sourcesArray.map((ds: any, index: number) => {
                        let bindingType: LayerBindingType = 'device'
                        if (ds.type === 'metric' || ds.type === 'telemetry') bindingType = 'metric'
                        else if (ds.type === 'command') bindingType = 'command'

                        const existingBinding = (config.bindings as LayerBinding[])?.find(b => {
                          if (!b.dataSource) return false
                          const bDs = b.dataSource as any
                          return getSourceId(bDs) === getSourceId(ds) &&
                            bDs.metricId === ds.metricId &&
                            bDs.property === ds.property &&
                            bDs.command === ds.command
                        })

                        const generateBindingId = () => {
                          if (ds.type === 'metric' || ds.type === 'telemetry') {
                            return `${bindingType}-${getSourceId(ds)}-${ds.metricId || ds.property || index}`
                          } else if (ds.type === 'command') {
                            return `${bindingType}-${getSourceId(ds)}-${ds.command}`
                          } else {
                            return `${bindingType}-${getSourceId(ds)}-${index}`
                          }
                        }

                        const baseBinding = existingBinding || {
                          id: generateBindingId(),
                          position: { x: 50, y: 50 },
                        }

                        return {
                          ...baseBinding,
                          id: existingBinding?.id || generateBindingId(),
                          type: bindingType,
                          icon: bindingType,
                          name: (ds.type === 'metric' || ds.type === 'telemetry')
                            ? (ds.metricId || ds.property || t('visualDashboard.metricIndex', { index: index + 1 }))
                            : ds.type === 'command'
                              ? `${getSourceId(ds) || ''} → ${ds.command || ''}`
                              : (getSourceId(ds) || t('visualDashboard.deviceIndex', { index: index + 1 })),
                          dataSource: ds,
                          position: existingBinding?.position || baseBinding.position,
                        } as LayerBinding
                      })

                      const existingTextIconBindings = (config.bindings as LayerBinding[])?.filter(b => {
                        if (b.type === 'text' || b.type === 'icon') return true
                        const ds = b.dataSource as any
                        if (ds && getSourceId(ds)) {
                          return !sourcesArray.some((s: any) => getSourceId(s) === getSourceId(ds))
                        }
                        return false
                      }) || []

                      updateConfig('bindings')([...newBindings, ...existingTextIconBindings])
                    }}
                    allowedTypes={['device', 'metric', 'command', 'extension']}
                    maxSources={20}
                    title={t('visualDashboard.layerItemBinding')}
                  />

                  <div className="flex items-center justify-between">
                    <div>
                      <h3 className="text-sm font-medium">{t('visualDashboard.layerItemBinding')}</h3>
                      <p className="text-xs text-muted-foreground mt-1">
                        {t('visualDashboard.manageLayerItems')}
                      </p>
                    </div>
                    <Button
                      type="button"
                      variant="default"
                      size="sm"
                      onClick={() => {
                        const latestDashboard = useStore.getState().currentDashboard
                        const latestComponent = latestDashboard?.components.find(c => c.id === selectedComponent?.id)
                        const latestBindings = (latestComponent as any)?.config?.bindings as LayerBinding[] || []
                        setLayerEditorBindings(latestBindings)
                        setLayerEditorOpen(true)
                      }}
                    >
                      <Layers className="h-4 w-4 mr-1" />
                      {t('visualDashboard.openLayerEditor')}
                    </Button>
                  </div>

                  {/* Bindings List - Grouped by Type */}
                  <div className="border rounded-lg overflow-hidden">
                    {(() => {
                      const latestDashboard = useStore.getState().currentDashboard
                      const latestComponent = latestDashboard?.components.find(c => c.id === selectedComponent?.id)
                      const displayBindings = (latestComponent as any)?.config?.bindings as LayerBinding[] || []

                      // Group by type
                      const groupedBindings = {
                        device: displayBindings.filter(b => b.type === 'device'),
                        metric: displayBindings.filter(b => b.type === 'metric'),
                        command: displayBindings.filter(b => b.type === 'command'),
                        text: displayBindings.filter(b => b.type === 'text'),
                        icon: displayBindings.filter(b => b.type === 'icon'),
                      }

                      const LAYER_TYPE_CONFIG = {
                        device: {
                          label: t('layerDisplay.device'),
                          color: 'bg-success',
                          textColor: 'text-success',
                          bgColor: 'bg-success-light',
                          borderColor: 'border-success-light dark:border-success-light',
                          icon: MapPin,
                          description: t('layerDisplay.deviceDesc')
                        },
                        metric: {
                          label: t('layerDisplay.metric'),
                          color: 'bg-accent-purple',
                          textColor: 'text-accent-purple',
                          bgColor: 'bg-accent-purple-light',
                          borderColor: 'border-accent-purple-light',
                          icon: Activity,
                          description: t('layerDisplay.metricDesc')
                        },
                        command: {
                          label: t('layerDisplay.command'),
                          color: 'bg-info',
                          textColor: 'text-info',
                          bgColor: 'bg-info-light',
                          borderColor: 'border-info',
                          icon: Zap,
                          description: t('layerDisplay.commandDesc')
                        },
                        text: {
                          label: t('layerDisplay.text'),
                          color: 'bg-muted-foreground',
                          textColor: 'text-muted-foreground',
                          bgColor: 'bg-muted',
                          borderColor: 'border-border',
                          icon: Type,
                          description: t('layerDisplay.textDesc')
                        },
                        icon: {
                          label: t('layerDisplay.icon'),
                          color: 'bg-accent-orange',
                          textColor: 'text-accent-orange',
                          bgColor: 'bg-accent-orange-light',
                          borderColor: 'border-accent-orange-light',
                          icon: Sparkles,
                          description: t('layerDisplay.iconDesc')
                        },
                      } as const

                      if (displayBindings.length === 0) {
                        return (
                          <div className="p-6 text-center text-muted-foreground">
                            <Layers className="h-8 w-8 mx-auto mb-2 opacity-50" />
                            <p className="text-sm">{t('visualDashboard.noLayerItems')}</p>
                            <p className="text-xs mt-1">{t('visualDashboard.addLayerItemHint')}</p>
                          </div>
                        )
                      }

                      return (Object.keys(groupedBindings) as Array<keyof typeof groupedBindings>).map(type => {
                        const typeBindings = groupedBindings[type]
                        if (typeBindings.length === 0) return null

                        const typeConfig = LAYER_TYPE_CONFIG[type]
                        const Icon = typeConfig.icon

                        return (
                          <div key={type} className="border-b last:border-b-0">
                            <div className={`px-3 py-2 ${typeConfig.bgColor} border-b ${typeConfig.borderColor} flex items-center justify-between`}>
                              <div className="flex items-center gap-2">
                                <div className={`w-5 h-5 rounded-full ${typeConfig.color} flex items-center justify-center`}>
                                  <Icon className="h-4 w-4 text-primary-foreground" />
                                </div>
                                <span className="text-sm font-medium">{typeConfig.label}</span>
                                <span className="text-xs text-muted-foreground">({typeBindings.length})</span>
                              </div>
                              <span className="text-xs text-muted-foreground">{typeConfig.description}</span>
                            </div>

                            <div className="divide-y">
                              {typeBindings.map((binding) => {
                                const positionText = binding.position && binding.position !== 'auto'
                                  ? `(${binding.position.x.toFixed(0)}%, ${binding.position.y.toFixed(0)}%)`
                                  : t('visualDashboard.center')

                                const ds = binding.dataSource as any
                                const deviceId = getSourceId(ds)
                                const metricId = ds?.metricId || ds?.property
                                const command = ds?.command

                                return (
                                  <div
                                    key={binding.id}
                                    className="flex items-center gap-3 p-3 hover:bg-muted-50 transition-colors"
                                  >
                                    <div className={`w-8 h-8 rounded-full flex items-center justify-center ${typeConfig.color}/20 ${typeConfig.textColor}`}>
                                      <Icon className="h-4 w-4" />
                                    </div>
                                    <div className="flex-1 min-w-0">
                                      <div className="text-sm font-medium truncate">{binding.name}</div>
                                      <div className="flex items-center gap-2 text-xs text-muted-foreground">
                                        <span>{positionText}</span>
                                        {deviceId && <span>• {deviceId.slice(0, 8)}...</span>}
                                        {metricId && <span>• {metricId}</span>}
                                        {command && <span>• {command}</span>}
                                      </div>
                                    </div>
                                  </div>
                                )
                              })}
                            </div>
                          </div>
                        )
                      })
                    })()}
                  </div>
                </div>
              ),
            },
          ],
        }

}
