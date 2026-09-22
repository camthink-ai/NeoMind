/**
 * Dynamic schema handler for extension, community, and custom components.
 *
 * Generates config UI from JSON Schema manifests, or falls back to
 * device-binding-only config when no schema is available.
 */

import React from 'react'
import { Field } from '@/components/ui/field'
import { Label } from '@/components/ui/label'
import { Input } from '@/components/ui/input'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import { Checkbox } from '@/components/ui/checkbox'
import type { ComponentConfigSchema, ConfigSection } from '@/components/dashboard/config/ComponentConfigBuilder'
import { DeviceBindingConfig } from '@/components/dashboard/config/DeviceBindingConfig'
import { dynamicRegistry } from '@/components/dashboard/registry/DynamicRegistry'
import { communityRegistry } from '@/components/dashboard/registry/CommunityRegistry'
import { useStore } from '@/store'
import { findDevice } from '@/lib/deviceUtils'
import type { SchemaContext, Updaters } from './types'

export function getDynamicSchema(
  componentType: string,
  config: any,
  ctx: SchemaContext,
  u: Updaters,
): ComponentConfigSchema | null {
  const { updateConfig, updateDataSource } = u

  // Check if this is an extension or community component
  const extensionDto = dynamicRegistry.getMeta(componentType)
  const communityMeta = communityRegistry.getMeta(componentType)
  const meta = extensionDto || communityMeta
  const schemaSource = extensionDto?.config_schema?.properties
    ? extensionDto
    : communityMeta?.config_schema?.properties
      ? communityMeta
      : null

  // ── ConfigPanel: community/extension bundle exports a custom config UI ──
  const globalName = meta?.global_name
  if (globalName) {
    const bundleGlobal = (window as any)[globalName]
    if (bundleGlobal?.ConfigPanel) {
      const ConfigPanelComponent = bundleGlobal.ConfigPanel
      const displaySections: ConfigSection[] = [
        {
          type: 'custom' as const,
          render: () => React.createElement(ConfigPanelComponent, {
            config,
            onChange: (key: string, value: any) => updateConfig(key)(value),
          }),
        },
      ]

      // Device binding section (if component requires it)
      if (communityMeta?.has_device_binding) {
        displaySections.push({
          type: 'custom' as const,
          render: () => (
            <DeviceBindingConfig
              deviceId={config.deviceBinding?.deviceId}
              deviceTypeFilter={communityMeta.device_type_filter}
              onChange={(deviceId) => {
                updateConfig('deviceBinding')({ deviceId: deviceId || undefined })
              }}
            />
          ),
        })
      }

      // Data source section (if component supports it)
      let dataSourceSections: ConfigSection[] = []
      if (meta.has_data_source) {
        const dsAllowedTypes = (meta.data_source_allowed_types || ['device-metric', 'extension', 'extension-command', 'ai']) as any
        const maxDs = meta.max_data_sources ?? 1
        dataSourceSections = [
          {
            type: 'data-source' as const,
            props: {
              dataSource: config.dataSource,
              onChange: updateDataSource,
              allowedTypes: dsAllowedTypes,
              multiple: maxDs > 1,
              maxSources: maxDs,
            },
          },
        ]
      }

      // AdvancedPanel → Advanced tab for complex/component-specific config
      const advancedSections: ConfigSection[] = []
      if (bundleGlobal.AdvancedPanel) {
        // Get device's current image from store (populated by WebSocket) for ROI editing
        const boundDeviceId = config.deviceBinding?.deviceId
        const storeState = useStore.getState()
        const boundDevice = boundDeviceId ? findDevice(storeState.devices, boundDeviceId) : null
        // Check both device.current_values and deviceTelemetry map
        const deviceValues = (boundDevice?.current_values || storeState.deviceTelemetry?.[boundDeviceId || ''] || {}) as Record<string, unknown>
        const getVal = (obj: any, ...paths: string[]) => {
          for (const p of paths) { let v = obj; for (const k of p.split('.')) { v = v?.[k]; if (!v) break; } if (v && typeof v === 'string') return v; }
          return undefined
        }
        const deviceImage = getVal(deviceValues, 'image', 'values.image', 'photo', 'values.photo', 'imageUrl', 'values.imageUrl', 'picture', 'values.picture')
        const deviceImageSrc = deviceImage
          ? (deviceImage.startsWith('data:') ? deviceImage : 'data:image/jpeg;base64,' + deviceImage)
          : undefined

        advancedSections.push({
          type: 'custom' as const,
          render: () => {
            return React.createElement(bundleGlobal.AdvancedPanel, {
              config,
              onChange: (key: string, value: any) => {
                return updateConfig(key)(value)
              },
              deviceImageSrc,
            })
          },
        })
      }

      return { displaySections, dataSourceSections, advancedSections }
    }
  }

  if (schemaSource?.config_schema?.properties) {
    // Generate config UI from JSON Schema (extension or community)
    const properties = schemaSource.config_schema.properties
    const uiHints = schemaSource.config_schema.ui_hints
    const fieldOrder = uiHints?.field_order || Object.keys(properties)

    // Visibility rules: check if a field should be visible based on current config values
    const isFieldVisible = (fieldName: string): boolean => {
      if (!uiHints?.visibility_rules) return true
      const rules = uiHints.visibility_rules as Array<{ field: string; condition: string; value: any; then_show?: string[]; then_hide?: string[] }>
      for (const rule of rules) {
        if (rule.then_show?.includes(fieldName)) {
          const ruleValue = config[rule.field] ?? schemaSource.default_config?.[rule.field]
          let show = false
          switch (rule.condition) {
            case 'equals': show = ruleValue === rule.value; break
            case 'not_equals': show = ruleValue !== rule.value; break
            case 'contains': show = Array.isArray(ruleValue) && ruleValue.includes(rule.value); break
            case 'empty': show = !ruleValue || (Array.isArray(ruleValue) && ruleValue.length === 0); break
            case 'not_empty': show = !!ruleValue && (!Array.isArray(ruleValue) || ruleValue.length > 0); break
          }
          if (show) return true
        }
        if (rule.then_hide?.includes(fieldName)) {
          const ruleValue = config[rule.field] ?? schemaSource.default_config?.[rule.field]
          let hide = false
          switch (rule.condition) {
            case 'equals': hide = ruleValue === rule.value; break
            case 'not_equals': hide = ruleValue !== rule.value; break
          }
          if (hide) return false
        }
      }
      // If field appears in any then_show rule but no rule matched, it's hidden
      const appearsInThenShow = rules.some(r => r.then_show?.includes(fieldName))
      return !appearsInThenShow
    }

    const displaySections: ConfigSection[] = [
      {
        type: 'custom' as const,
        render: () => (
          <div className="space-y-3">
            {fieldOrder.filter(key => properties[key] && isFieldVisible(key)).map((key) => {
              const propDef = properties[key]
              const propValue = config[key] ?? schemaSource.default_config?.[key] ?? propDef.default

              const handleChange = (value: any) => {
                updateConfig(key)(value)
              }

              // Render based on property type
              const fieldLabel = propDef.title || propDef.description || key

              switch (propDef.type) {
                case 'boolean':
                  return (
                    <label key={key} className="flex items-center gap-2 cursor-pointer">
                      <Checkbox
                        checked={propValue ?? false}
                        onCheckedChange={(checked) => handleChange(!!checked)}
                      />
                      <span className="text-sm font-medium">{fieldLabel}</span>
                    </label>
                  )

                case 'number':
                  return (
                    <Field key={key}>
                      <Label>{fieldLabel}</Label>
                      <Input
                        type="number"
                        value={propValue ?? 0}
                        onChange={(e) => handleChange(Number(e.target.value))}
                        min={propDef.minimum}
                        max={propDef.maximum}
                        step={propDef.type === 'number' ? (propDef.multipleOf || 1) : undefined}
                        className="h-9"
                      />
                    </Field>
                  )

                case 'integer':
                  return (
                    <Field key={key}>
                      <Label>{fieldLabel}</Label>
                      <Input
                        type="number"
                        value={propValue ?? 0}
                        onChange={(e) => handleChange(Math.floor(Number(e.target.value)))}
                        min={propDef.minimum}
                        max={propDef.maximum}
                        step="1"
                        className="h-9"
                      />
                    </Field>
                  )

                case 'string':
                  if (propDef.enum) {
                    // Select dropdown for enum values
                    const enumLabels = propDef.enumTitles || propDef.enum
                    return (
                      <Field key={key}>
                        <Label>{fieldLabel}</Label>
                        <Select
                          value={propValue ?? propDef.default ?? propDef.enum[0]}
                          onValueChange={(value) => handleChange(value)}
                        >
                          <SelectTrigger className="w-full h-9">
                            <SelectValue />
                          </SelectTrigger>
                          <SelectContent>
                            {propDef.enum.map((enumValue: string, idx: number) => (
                              <SelectItem key={enumValue} value={enumValue}>
                                {enumLabels[idx]}
                              </SelectItem>
                            ))}
                          </SelectContent>
                        </Select>
                      </Field>
                    )
                  }
                  return (
                    <Field key={key}>
                      <Label>{fieldLabel}</Label>
                      <Input
                        value={propValue ?? ''}
                        onChange={(e) => handleChange(e.target.value)}
                        placeholder={propDef.description || fieldLabel}
                        className="h-9"
                      />
                    </Field>
                  )

                case 'array':
                  return (
                    <Field key={key}>
                      <Label>{fieldLabel}</Label>
                      <Input
                        value={Array.isArray(propValue) ? propValue.join(', ') : ''}
                        onChange={(e) => handleChange(e.target.value.split(',').map((s: string) => s.trim()))}
                        placeholder="Comma-separated values"
                        className="h-9"
                      />
                    </Field>
                  )

                default:
                  return null
              }
            })}
          </div>
        ),
      },
    ]

    // Add device binding section if component requires it
    if (communityMeta?.has_device_binding) {
      displaySections.push({
        type: 'custom' as const,
        render: () => (
          <DeviceBindingConfig
            deviceId={config.deviceBinding?.deviceId}
            deviceTypeFilter={communityMeta.device_type_filter}
            onChange={(deviceId) => {
              updateConfig('deviceBinding')({ deviceId: deviceId || undefined })
            }}
          />
        ),
      })
    }

    // Add data source section if component supports it
    let dataSourceSections: ConfigSection[] = []
    if (schemaSource.has_data_source) {
      const dsAllowedTypes = (schemaSource.data_source_allowed_types || ['device-metric', 'extension', 'extension-command', 'ai']) as any
      const maxDs = schemaSource.max_data_sources ?? 1
      dataSourceSections = [
        {
          type: 'data-source' as const,
          props: {
            dataSource: config.dataSource,
            onChange: updateDataSource,
            allowedTypes: dsAllowedTypes,
            multiple: maxDs > 1,
            maxSources: maxDs,
          },
        },
      ]
    }

    return {
      displaySections,
      dataSourceSections,
      styleSections: [],
    }
  }

  // Community component with device binding but no config_schema
  if (communityMeta?.has_device_binding) {
    return {
      displaySections: [
        {
          type: 'custom' as const,
          render: () => (
            <DeviceBindingConfig
              deviceId={config.deviceBinding?.deviceId}
              deviceTypeFilter={communityMeta.device_type_filter}
              onChange={(deviceId) => {
                updateConfig('deviceBinding')({ deviceId: deviceId || undefined })
              }}
            />
          ),
        },
      ],
      dataSourceSections: [],
      styleSections: [],
    }
  }

  return null
}
