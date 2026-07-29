/**
 * TransformBuilderSplit Component
 *
 * Split-workspace dialog for creating/editing data transforms.
 * Uses BuilderShell with emerald accent: config rail + code workspace + test strip.
 *
 * @module automation
 */

import React, { useState, useEffect, useCallback, useMemo } from 'react'
import { useTranslation } from 'react-i18next'
import { api } from '@/lib/api'
import { cn } from '@/lib/utils'
import { cardPadded } from '@/design-system/tokens/size'
import { textNano } from "@/design-system/tokens/typography"
import { useIsMobile } from '@/hooks/useMobile'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Textarea } from '@/components/ui/textarea'
import { CodeEditor } from '@/components/ui/code-editor'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { Badge } from '@/components/ui/badge'
import { Checkbox } from '@/components/ui/checkbox'
import { Switch } from '@/components/ui/switch'
import {
  Code,
  Loader2,
  Play,
  Database,
  FlaskConical,
  Check,
  Plus,
  Puzzle,
  FileCode,
  Zap,
} from 'lucide-react'
import type {
  TransformAutomation,
  TransformScope,
  ExtensionDataType,
} from '@/types'
import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from '@/components/ui/popover'
// Builder shell + field primitives
import { BuilderShell } from './dialog/BuilderShell'
import { Field, FieldLabel, FieldMessage, FieldDescription } from '@/components/ui/field'
import { LoadingState } from '@/components/shared/LoadingState'

// ============================================================================
// Types
// ============================================================================

interface TransformBuilderProps {
  open: boolean
  onOpenChange: (open: boolean) => void
  transform?: TransformAutomation | null
  devices: Array<{ id: string; name: string; device_type?: string }>
  deviceTypes?: Array<{ device_type: string; name?: string }>
  onSave: (data: Partial<TransformAutomation>) => void
}

type ScopeType = 'global' | 'device_type' | 'device'

interface FormErrors {
  name?: string
  code?: string
  outputPrefix?: string
  scopeValue?: string
}

// Selected extension data source
interface SelectedExtensionSource {
  extension_id: string
  extension_name: string
  command: string
  command_display_name?: string
  field: string
  display_name: string
  data_type: ExtensionDataType
  unit?: string
}

// Extension data source with nested structure
interface ExtensionDataSourceGroup {
  extension_id: string
  extension_name: string
  commands: Array<{
    command: string
    command_display_name: string
    fields: Array<{
      field: string
      display_name: string
      data_type: ExtensionDataType
      unit?: string
    }>
  }>
}

// Extension command for invocation
interface ExtensionCommandInfo {
  extension_id: string
  extension_name: string
  command_id: string
  command_name: string
  display_name: string
  description: string
  // Parameters extracted from input_schema
  parameters: Array<{
    name: string
    display_name: string
    data_type: string
    required: boolean
    default_value?: unknown
    description?: string
  }>
}

// Code templates for common data transformations
// Note: names will be translated via i18n
const CODE_TEMPLATES = [
  {
    key: 'simple',
    nameKey: 'templates.simple',
    code: '// Device metric: input.temperature (Celsius)\nreturn {\n  temp_f: (input.temperature || 0) * 9 / 5 + 32,\n  status: (input.temperature || 0) > 30 ? \'hot\' : \'normal\'\n};',
  },
  {
    key: 'extensionOnly',
    nameKey: 'templates.extensionOnly',
    code: '// Use extension data from selected sources\n// Access via: input.extensions.{ext_id}.{command}.{field}\nconst ext = input.extensions || {};\n\n// Example: weather extension data\nconst weather = ext.weather?.get_current || {};\nreturn {\n  outdoor_temp: weather.temp_f || 0,\n  condition: weather.condition || \'unknown\',\n  has_data: Object.keys(weather).length > 0\n};',
  },
  {
    key: 'combined',
    nameKey: 'templates.combined',
    code: '// Combine device metrics and extension data\nconst deviceTemp = input.temperature || 0;\nconst ext = input.extensions || {};\n\n// Get extension data (check if exists first)\nconst extData = ext[Object.keys(ext)[0]] || {};\nconst firstCommand = extData[Object.keys(extData)[0]] || {};\nconst extTemp = firstCommand.temp_f || deviceTemp;\n\nreturn {\n  // Average of device and extension temperature\n  temp_avg: (deviceTemp + extTemp * 5/9) / 2,\n  // Status based on combined reading\n  is_hot: deviceTemp > 30 || extTemp > 90,\n  // Difference\n  temp_diff: Math.abs(deviceTemp - extTemp * 5 / 9)\n};',
  },
  {
    key: 'batteryStatus',
    nameKey: 'templates.batteryStatus',
    code: '// Battery level status calculation\nconst battery = input.battery || input.batt || input.level || 0;\nreturn {\n  battery_percent: Math.min(100, Math.max(0, battery)),\n  battery_status: battery > 80 ? \'good\' : battery > 20 ? \'medium\' : \'low\',\n  needs_charging: battery < 20\n};',
  },
  {
    key: 'statusCheck',
    nameKey: 'templates.statusCheck',
    code: '// Multi-level status check\nconst value = input.value || input.val || input.temperature || 0;\nreturn {\n  status: value > 100 ? \'critical\' : value > 80 ? \'warning\' : \'normal\',\n  is_alert: value > 100,\n  severity: value > 100 ? 3 : value > 80 ? 2 : 1,\n  normalized: Math.min(100, Math.max(0, value))\n};',
  },
  {
    key: 'passThrough',
    nameKey: 'templates.passThrough',
    code: '// Pass through all input data unchanged\nreturn input;',
  },
  {
    key: 'extensionInvoke',
    nameKey: 'templates.extensionInvoke',
    code: '// Call an extension action to process data\n// Replace {extension_id} and {command} with actual values\n// Click on an action in the Variables Panel to auto-generate code\nconst result = extensions_invoke(\'{extension_id}\', \'{command}\', {\n  data: input,\n})\n\nreturn result',
  },
]

// ============================================================================
// Helper: extract parameters from JSON schema
// ============================================================================

function extractParametersFromSchema(schema: Record<string, unknown> | undefined): ExtensionCommandInfo['parameters'] {
  if (!schema || typeof schema !== 'object') return []
  const params: ExtensionCommandInfo['parameters'] = []
  const properties = schema.properties as Record<string, unknown> | undefined
  const required = (schema.required as string[]) || []
  if (!properties) return params
  for (const [name, propSchema] of Object.entries(properties)) {
    if (typeof propSchema !== 'object' || propSchema === null) continue
    const prop = propSchema as Record<string, unknown>
    let dataType = 'string'
    const type = prop.type as string | undefined
    if (type === 'number' || type === 'integer') dataType = 'number'
    else if (type === 'boolean') dataType = 'boolean'
    else if (type === 'array') dataType = 'array'
    else if (type === 'object') dataType = 'object'
    params.push({
      name,
      display_name: (prop.title as string) || name,
      data_type: dataType,
      required: required.includes(name),
      default_value: prop.default,
      description: prop.description as string | undefined,
    })
  }
  return params
}

function getTypeColor(type: string) {
  switch (type) {
    case 'number': case 'integer': case 'float': return 'text-info'
    case 'string': return 'text-success'
    case 'boolean': return 'text-accent-purple'
    case 'object': return 'text-accent-orange'
    case 'array': return 'text-accent-cyan'
    case 'binary': return 'text-warning'
    default: return 'text-muted-foreground'
  }
}

function getTypeIcon(type: string) {
  switch (type) {
    case 'number': case 'integer': case 'float': return '#'
    case 'string': return '"'
    case 'boolean': return 'TF'
    case 'object': return '{}'
    case 'array': return '[]'
    case 'binary': return 'BIN'
    default: return '?'
  }
}

// ============================================================================
// VariablesRail — single unified rail replacing the old two Tabs blocks
// ============================================================================

interface VariablesRailProps {
  deviceTypeMetrics?: Array<{ name: string; display_name: string; data_type: string; unit?: string }>
  extensionSources: SelectedExtensionSource[]
  onExtensionSourcesChange: (sources: SelectedExtensionSource[]) => void
  onInsertVariable: (variable: string) => void
  tBuilder: (key: string) => string
  t: (key: string, params?: Record<string, unknown>) => string
}

function VariablesRail({
  deviceTypeMetrics,
  extensionSources,
  onExtensionSourcesChange,
  onInsertVariable,
  tBuilder,
  t,
}: VariablesRailProps) {
  const isMobile = useIsMobile()
  const [extensions, setExtensions] = useState<ExtensionDataSourceGroup[]>([])
  const [extensionCommands, setExtensionCommands] = useState<ExtensionCommandInfo[]>([])
  const [loadingExtensions, setLoadingExtensions] = useState(true)
  const [extensionPopoverOpen, setExtensionPopoverOpen] = useState(false)
  const [varTab, setVarTab] = useState<'device' | 'extension' | 'actions'>('device')

  // Fetch extension data sources and commands on mount
  useEffect(() => {
    const fetchSources = async () => {
      setLoadingExtensions(true)
      try {
        const [allSources, extList] = await Promise.all([
          api.listAllDataSources(),
          api.listExtensions()
        ])

        // Filter only extension data sources (exclude transform data sources)
        const extSources = allSources.filter(
          (source): source is import('@/types').ExtensionDataSourceInfo =>
            'extension_id' in source
        )

        // Group data sources by extension
        const groups: Record<string, ExtensionDataSourceGroup> = {}
        for (const source of extSources) {
          const key = source.extension_id
          if (!groups[key]) {
            const ext = extList.find(e => e.id === source.extension_id)

            groups[key] = {
              extension_id: source.extension_id,
              extension_name: ext?.name || source.extension_id,
              commands: [], // For V2, we use a single "metrics" group
            }
          }

          // In V2, command is empty, use "metrics" as the display name
          const cmdKey = source.command || 'metrics'
          let cmdGroup = groups[key].commands.find(c => c.command === cmdKey)
          if (!cmdGroup) {
            cmdGroup = {
              command: cmdKey,
              command_display_name: 'Metrics',
              fields: [],
            }
            groups[key].commands.push(cmdGroup)
          }

          cmdGroup.fields.push({
            field: source.field,
            display_name: source.display_name,
            data_type: source.data_type as ExtensionDataType,
            unit: source.unit,
          })
        }

        setExtensions(Object.values(groups))

        // Collect extension commands (for invocation) with parameter info
        const commands: ExtensionCommandInfo[] = []
        for (const ext of extList) {
          for (const cmd of ext.commands || []) {
            // Skip metrics commands (data sources) - only show executable commands
            if (cmd.id === 'metrics' || cmd.id === 'get_current') {
              continue
            }

            // Extract parameters from input_schema
            const parameters = extractParametersFromSchema(cmd.input_schema)

            commands.push({
              extension_id: ext.id,
              extension_name: ext.name,
              command_id: cmd.id,
              command_name: cmd.id,
              display_name: cmd.display_name || cmd.id,
              description: cmd.description || '',
              parameters,
            })
          }
        }
        setExtensionCommands(commands)
      } catch (err) {
        console.error('Failed to load extension data sources:', err)
      } finally {
        setLoadingExtensions(false)
      }
    }

    fetchSources()
  }, [])

  const isSourceSelected = (extId: string, field: string) => {
    return extensionSources.some(s => s.extension_id === extId && s.field === field)
  }

  const toggleSource = (extId: string, extName: string, field: string, display: string, dataType: ExtensionDataType, unit: string | undefined) => {
    const currentSources = extensionSources
    const key = `${extId}/${field}`
    if (isSourceSelected(extId, field)) {
      onExtensionSourcesChange(currentSources.filter(s => `${s.extension_id}/${s.field}` !== key))
    } else {
      onExtensionSourcesChange([...currentSources, {
        extension_id: extId,
        extension_name: extName,
        command: 'metrics', // V2: fixed value
        command_display_name: 'Metrics',
        field,
        display_name: display,
        data_type: dataType,
        unit,
      }])
    }
  }

  // Generate code to call an extension command
  const handleInvokeCommand = (cmdInfo: ExtensionCommandInfo) => {
    // Build parameters object with all params
    let paramsCode = '  data: input'
    if (cmdInfo.parameters.length > 0) {
      const otherParams = cmdInfo.parameters
        .filter(p => p.name !== 'data')
        .map(p => {
          const comment = p.required ? '' : ` // optional${p.description ? ` - ${p.description}` : ''}`
          const value = p.default_value !== undefined ? JSON.stringify(p.default_value) : `/* ${p.display_name} */`
          return `\n  ${p.name}: ${value},${comment}`
        })
        .join('')
      if (otherParams) {
        paramsCode += ',' + otherParams
      }
    }

    const hasParams = cmdInfo.parameters.length > 0
    const resultParam = hasParams ? 'result' : '{ processed: true }'

    const codeTemplate = `// Call ${cmdInfo.extension_name}: ${cmdInfo.display_name}\n// ${cmdInfo.description || 'Execute extension command'}\nconst ${resultParam} = extensions_invoke('${cmdInfo.extension_id}', '${cmdInfo.command_name}', {\n${paramsCode}\n})\n\nreturn ${resultParam}`

    onInsertVariable(codeTemplate)
  }

  // Group extension sources by extension and command
  const groupedExtensions = useMemo(() => {
    const groups: Record<string, Record<string, SelectedExtensionSource[]>> = {}
    for (const source of extensionSources) {
      if (!groups[source.extension_id]) {
        groups[source.extension_id] = {}
      }
      if (!groups[source.extension_id][source.command]) {
        groups[source.extension_id][source.command] = []
      }
      groups[source.extension_id][source.command].push(source)
    }
    return groups
  }, [extensionSources])

  // --- Render ---

  if (loadingExtensions) {
    return (
      <div className={cn(
        "h-full min-h-0",
        isMobile ? "w-full" : "w-72"
      )}>
        <LoadingState variant="page" />
      </div>
    )
  }

  const deviceCount = deviceTypeMetrics?.length || 0
  const tabs: Array<{ id: 'device' | 'extension' | 'actions'; label: string; count: number }> = [
    { id: 'device', label: tBuilder('device'), count: deviceCount },
    { id: 'extension', label: tBuilder('extension'), count: extensionSources.length },
    ...(extensionCommands.length > 0
      ? [{ id: 'actions' as const, label: tBuilder('extensionActions'), count: extensionCommands.length }]
      : []),
  ]
  // Guard: if active tab was removed (e.g. actions tab hidden), fall back to device
  const activeTab = tabs.some(t => t.id === varTab) ? varTab : 'device'

  return (
    <div className={cn(
      "flex flex-col h-full min-h-0",
      isMobile ? "w-full" : "w-72"
    )}>
      {/* Unified header: title + segmented tabs + actions */}
      <div className={cn(
        "border-b flex items-center gap-1.5 shrink-0",
        isMobile ? "px-3 py-2" : "px-2.5 py-1.5"
      )}>
        <Database className={cn("text-info shrink-0", isMobile ? "h-4 w-4" : "h-3.5 w-3.5")} />
        <span className={cn(
          "font-medium min-w-0 truncate mr-auto",
          isMobile ? "text-sm" : "text-xs"
        )} title={tBuilder('availableVars')}>
          {tBuilder('availableVars')}
        </span>
        {/* Segmented tabs */}
        <div className="flex items-center gap-0.5 shrink-0">
          {tabs.map((tab) => (
            <button
              key={tab.id}
              type="button"
              onClick={() => setVarTab(tab.id)}
              className={cn(
                "flex items-center gap-1 px-1.5 h-6 text-[11px] font-medium rounded-md transition-colors",
                activeTab === tab.id
                  ? "bg-card text-foreground shadow-sm border border-border"
                  : "text-muted-foreground hover:text-foreground hover:bg-muted"
              )}
            >
              <span>{tab.label}</span>
              {tab.count > 0 && (
                <span className={cn(
                  "inline-flex items-center justify-center rounded-full min-w-[15px] h-[15px] text-[9px] leading-none px-1 font-semibold",
                  activeTab === tab.id
                    ? "bg-muted text-foreground"
                    : "bg-muted text-muted-foreground"
                )}>
                  {tab.count}
                </span>
              )}
            </button>
          ))}
        </div>
        {/* Select sources (extension tab only) */}
        {activeTab === 'extension' && (
          <Popover open={extensionPopoverOpen} onOpenChange={setExtensionPopoverOpen}>
            <PopoverTrigger asChild>
              <Button variant="ghost" size="sm" className="shrink-0 h-6 w-6 p-0" aria-label={tBuilder('selectSources')}>
                <Plus className="h-3.5 w-3.5" />
              </Button>
            </PopoverTrigger>
            <PopoverContent className="w-72 max-h-80 overflow-auto p-3" align="end">
              {extensions.length === 0 ? (
                <div className="text-center py-4 text-sm text-muted-foreground">
                  {tBuilder('noExtensionSources')}
                </div>
              ) : (
                <div className="space-y-3">
                  {extensions.map((ext) => (
                    <div key={ext.extension_id} className="space-y-1.5">
                      <div className="font-medium text-xs flex items-center gap-2">
                        <Puzzle className="h-4 w-4 text-accent-purple" />
                        {ext.extension_name}
                      </div>
                      {ext.commands.map((cmd) => (
                        <div key={cmd.command} className="ml-4 space-y-1">
                          {cmd.fields.map((field) => (
                            <div key={field.field} className="flex items-center gap-2 min-w-0 py-0.5">
                              <Checkbox
                                id={`field-${ext.extension_id}-${cmd.command}-${field.field}`}
                                checked={isSourceSelected(ext.extension_id, field.field)}
                                className="h-4 w-4 shrink-0"
                                onCheckedChange={() => toggleSource(
                                  ext.extension_id,
                                  ext.extension_name,
                                  field.field,
                                  field.display_name,
                                  field.data_type,
                                  field.unit
                                )}
                              />
                              <label
                                htmlFor={`field-${ext.extension_id}-${cmd.command}-${field.field}`}
                                className="text-xs text-muted-foreground cursor-pointer flex-1 min-w-0 truncate"
                                title={field.display_name}
                              >
                                {field.field}
                                {field.unit && <span className="ml-1 text-muted-foreground">({field.unit})</span>}
                              </label>
                            </div>
                          ))}
                        </div>
                      ))}
                    </div>
                  ))}
                </div>
              )}
            </PopoverContent>
          </Popover>
        )}
      </div>

      {/* Scrollable list */}
      <div className={cn(
        "overflow-y-auto overflow-x-hidden flex-1 min-h-0",
        isMobile ? "p-3" : "p-2"
      )}>
        {/* --- Device Metrics tab --- */}
        {activeTab === 'device' && (
          <>
            {deviceTypeMetrics && deviceTypeMetrics.length > 0 ? (
              <div className={cn("space-y-1", isMobile ? "space-y-2" : "")}>
                {deviceTypeMetrics.map((metric, idx) => (
                  <div
                    key={idx}
                    className={cn(
                      "flex items-center justify-between bg-background border rounded hover:bg-muted hover:border-border transition-all cursor-pointer group",
                      isMobile ? "px-4 py-3" : "px-2 py-1.5"
                    )}
                    onClick={() => {
                      // Add ?. for nested paths (e.g., "metadata.height" -> "metadata?.height")
                      const safePath = metric.name.split('.').join('?.')
                      onInsertVariable(`input.${safePath}`)
                    }}
                  >
                    <div className="flex items-center gap-2 min-w-0 flex-1">
                      <code className={cn(
                        "font-mono text-info truncate",
                        isMobile ? "text-sm" : "text-xs"
                      )}>
                        {metric.name}
                      </code>
                      {metric.display_name && metric.display_name !== metric.name && (
                        <span className={cn("text-muted-foreground truncate", isMobile ? "text-sm" : "text-xs")}>{metric.display_name}</span>
                      )}
                    </div>
                    <div className="flex items-center gap-1 flex-shrink-0">
                      {metric.unit && (
                        <span className={cn("text-muted-foreground", isMobile ? "text-sm" : "text-xs")}>{metric.unit}</span>
                      )}
                      <Badge variant="outline" className={cn(
                        'py-0',
                        isMobile ? 'h-7 px-2 text-sm' : 'h-5 px-1.5 text-xs',
                        getTypeColor(metric.data_type)
                      )}>
                        {getTypeIcon(metric.data_type)}
                      </Badge>
                    </div>
                  </div>
                ))}
              </div>
            ) : (
              <div className={cn(
                "text-center text-muted-foreground py-4 px-2",
                isMobile ? "text-sm" : "text-xs"
              )}>
                <Database className={cn("mx-auto mb-1.5 opacity-30", isMobile ? "h-8 w-8" : "h-6 w-6")} />
                <div>{tBuilder('noVariablesHint')}</div>
              </div>
            )}
          </>
        )}

        {/* --- Extension sources tab --- */}
        {activeTab === 'extension' && (
          <>
            {extensionSources.length > 0 ? (
              <div className={cn("space-y-2", isMobile ? "space-y-3" : "")}>
                {Object.entries(groupedExtensions).map(([extId, commands]) => {
                  const extName = extensionSources.find(s => s.extension_id === extId)?.extension_name || extId
                  return (
                    <div key={extId} className="border rounded bg-accent-purple-light overflow-hidden">
                      <div className={cn(
                        "border-b font-medium text-accent-purple",
                        isMobile ? "px-4 py-2.5 text-sm" : "px-2.5 py-1.5 text-xs"
                      )}>
                        {extName}
                      </div>
                      <div className={cn("space-y-1", isMobile ? "p-3" : "p-1.5")}>
                        {Object.entries(commands).map(([cmd, fields]) => (
                          <div key={cmd}>
                            <div className={cn("text-muted-foreground px-1 py-0.5", isMobile ? "text-sm" : "text-xs")}>{cmd}</div>
                            {fields.map((field, idx) => (
                              <div
                                key={idx}
                                className={cn(
                                  "flex items-center justify-between bg-background border rounded hover:bg-muted hover:border-border transition-all cursor-pointer group",
                                  isMobile ? "px-4 py-3" : "px-2 py-1.5"
                                )}
                                onClick={() => onInsertVariable(`input.extensions?.${extId}?.${field.field}`)}
                              >
                                <div className="flex items-center gap-2 min-w-0 flex-1">
                                  <code className={cn(
                                    "font-mono text-accent-purple truncate",
                                    isMobile ? "text-sm" : "text-xs"
                                  )}>
                                    {field.field}
                                  </code>
                                  {field.display_name !== field.field && (
                                    <span className={cn("text-muted-foreground truncate", isMobile ? "text-sm" : "text-xs")}>{field.display_name}</span>
                                  )}
                                </div>
                                <div className="flex items-center gap-1 flex-shrink-0">
                                  {field.unit && (
                                    <span className={cn("text-muted-foreground", isMobile ? "text-sm" : "text-xs")}>{field.unit}</span>
                                  )}
                                  <Badge variant="outline" className={cn(
                                    'py-0',
                                    isMobile ? 'h-7 px-2 text-sm' : 'h-5 px-1.5 text-xs',
                                    getTypeColor(field.data_type)
                                  )}>
                                    {getTypeIcon(field.data_type)}
                                  </Badge>
                                </div>
                              </div>
                            ))}
                          </div>
                        ))}
                      </div>
                    </div>
                  )
                })}
              </div>
            ) : (
              <div className={cn(
                "text-center text-muted-foreground py-4 px-2",
                isMobile ? "text-sm" : "text-xs"
              )}>
                <Puzzle className={cn("mx-auto mb-1.5 opacity-30", isMobile ? "h-8 w-8" : "h-6 w-6")} />
                <div>{tBuilder('noSourcesSelectedHint')}</div>
              </div>
            )}
          </>
        )}

        {/* --- Extension actions tab --- */}
        {activeTab === 'actions' && (
          <>
            {extensionCommands.length > 0 ? (
              <div className={cn("space-y-1", isMobile ? "space-y-2" : "")}>
                {extensionCommands.map((cmd, idx) => (
                  <div
                    key={idx}
                    className={cn(
                      "flex items-center justify-between bg-background border rounded hover:bg-muted hover:border-border transition-all cursor-pointer group",
                      isMobile ? "px-4 py-3" : "px-2 py-1.5"
                    )}
                    onClick={() => handleInvokeCommand(cmd)}
                    title={cmd.description || undefined}
                  >
                    <div className="flex items-center gap-2 min-w-0 flex-1">
                      <Zap className={cn("text-warning shrink-0", isMobile ? "h-4 w-4" : "h-3.5 w-3.5")} />
                      <div className="min-w-0 flex flex-col">
                        <code className={cn("font-mono text-warning truncate", isMobile ? "text-sm" : "text-xs")}>
                          {cmd.display_name}
                        </code>
                        <span className={cn("text-muted-foreground truncate", isMobile ? "text-sm" : "text-xs")}>
                          {cmd.extension_name} · {cmd.command_id}
                          {cmd.parameters.length > 0 && (
                            <span className="text-muted-foreground"> ({cmd.parameters.length} {tBuilder('params')})</span>
                          )}
                        </span>
                      </div>
                    </div>
                    <Badge variant="outline" className={cn(
                      'py-0 shrink-0 text-warning border-warning',
                      isMobile ? 'h-7 px-2 text-sm' : 'h-5 px-1.5 text-xs'
                    )}>
                      {tBuilder('call')}
                    </Badge>
                  </div>
                ))}
              </div>
            ) : (
              <div className={cn(
                "text-center text-muted-foreground py-4 px-2",
                isMobile ? "text-sm" : "text-xs"
              )}>
                <Zap className={cn("mx-auto mb-1.5 opacity-30", isMobile ? "h-8 w-8" : "h-6 w-6")} />
                <div>{tBuilder('noActionsHint')}</div>
              </div>
            )}
          </>
        )}
      </div>
    </div>
  )
}

// ============================================================================
// TestStrip — compact test panel
// ============================================================================

interface TestStripProps {
  jsCode: string
  testInput: string
  onTestInputChange: (v: string) => void
  testOutput: string
  testError: string
  testRunning: boolean
  onTest: () => void
  onClearTest: () => void
  deviceTypeMetrics?: Array<{ name: string; display_name: string; data_type: string; unit?: string }>
  extensionSources: SelectedExtensionSource[]
  tBuilder: (key: string) => string
}

function TestStrip({
  jsCode,
  testInput,
  onTestInputChange,
  testOutput,
  testError,
  testRunning,
  onTest,
  onClearTest,
  deviceTypeMetrics,
  extensionSources,
  tBuilder,
}: TestStripProps) {
  const generateMockData = useCallback(() => {
    const mockData: Record<string, unknown> & { extensions?: Record<string, unknown> } = {}

    // Generate device metrics mock data
    if (deviceTypeMetrics && deviceTypeMetrics.length > 0) {
      for (const metric of deviceTypeMetrics) {
        switch (metric.data_type) {
          case 'integer':
            mockData[metric.name] = Math.floor(Math.random() * 100)
            break
          case 'float':
            mockData[metric.name] = parseFloat((Math.random() * 100).toFixed(2))
            break
          case 'string':
            mockData[metric.name] = `sample_${metric.name}`
            break
          case 'boolean':
            mockData[metric.name] = Math.random() > 0.5
            break
          case 'array':
            mockData[metric.name] = [
              Math.floor(Math.random() * 100),
              parseFloat((Math.random() * 100).toFixed(2)),
              `sample_${metric.name}`
            ]
            break
          default:
            mockData[metric.name] = null
        }
      }
    } else {
      // Default mock data
      mockData.temperature = 25
      mockData.humidity = 60
    }

    // Add extension data mock (V2: flat structure without command layer)
    if (extensionSources && extensionSources.length > 0) {
      const extensions: Record<string, Record<string, unknown>> = {}
      mockData.extensions = extensions
      for (const source of extensionSources) {
        if (!extensions[source.extension_id]) {
          extensions[source.extension_id] = {}
        }
        switch (source.data_type) {
          case 'integer':
            extensions[source.extension_id][source.field] = Math.floor(Math.random() * 100)
            break
          case 'number':
            extensions[source.extension_id][source.field] = parseFloat((Math.random() * 100).toFixed(2))
            break
          case 'boolean':
            extensions[source.extension_id][source.field] = Math.random() > 0.5
            break
          case 'string':
            extensions[source.extension_id][source.field] = `sample_${source.field}`
            break
          default:
            extensions[source.extension_id][source.field] = null
        }
      }
    }

    onTestInputChange(JSON.stringify(mockData, null, 2))
  }, [deviceTypeMetrics, extensionSources, onTestInputChange])

  return (
    <div className={cn(cardPadded, "space-y-3")}>
      {/* Title + inline actions */}
      <div className="flex items-center justify-between gap-2">
        <div className="flex items-center gap-2 min-w-0">
          <Play className="h-4 w-4 text-muted-foreground shrink-0" />
          <span className="text-sm font-medium truncate">{tBuilder('testPanel')}</span>
        </div>
        <div className="flex items-center gap-1.5 shrink-0">
          <Button
            variant="outline"
            className="h-7 px-2.5 text-xs"
            onClick={generateMockData}
          >
            <FlaskConical className="h-3.5 w-3.5 mr-1" />
            {tBuilder('generateMock')}
          </Button>
          <Button
            className="h-7 px-2.5 text-xs"
            onClick={onTest}
            disabled={!jsCode || testRunning}
          >
            {testRunning ? <Loader2 className="h-3.5 w-3.5 animate-spin" /> : <Play className="h-3.5 w-3.5 mr-1" />}
            {tBuilder('run')}
          </Button>
        </div>
      </div>

      {/* Input */}
      <Textarea
        value={testInput}
        onChange={e => onTestInputChange(e.target.value)}
        placeholder='{"temperature": 25}'
        className="font-mono text-xs resize-none h-20 md:h-16"
      />

      {/* Output / Error row */}
      {(testOutput || testError) && (
        <div>
          <div className="flex items-center gap-2 mb-1 justify-between">
            <div className="flex items-center gap-2 min-w-0">
              <span className="text-xs text-muted-foreground">{tBuilder('outputData')}</span>
              {testError ? (
                <span className="text-xs text-error flex items-center gap-1">
                  <span className="inline-block w-1.5 h-1.5 rounded-full bg-error" />
                  {tBuilder('testFailed')}
                </span>
              ) : (
                <span className="text-xs text-success flex items-center gap-1">
                  <Check className="h-3 w-3" />
                  {tBuilder('testSuccess')}
                </span>
              )}
            </div>
            <Button
              variant="ghost"
              className="h-6 px-2 text-xs shrink-0"
              onClick={onClearTest}
            >
              {tBuilder('clear')}
            </Button>
          </div>
          <div className="rounded-md p-2 max-h-40 overflow-auto">
            {testError && (
              <div className="p-1.5 bg-muted border border-error rounded text-xs text-error font-mono">
                {testError}
              </div>
            )}
            {testOutput && !testError && (
              <pre className="text-xs font-mono text-muted-foreground whitespace-pre-wrap break-all">
                {testOutput}
              </pre>
            )}
          </div>
        </div>
      )}
    </div>
  )
}

// ============================================================================
// TransformWorkspace — local component for the BuilderShell workspace slot
// ============================================================================

interface TransformWorkspaceProps {
  jsCode: string
  onCodeChange: (v: string) => void
  templates: Array<{ key: string; nameKey: string; code: string }>
  onApplyTemplate: (code: string) => void
  deviceTypeMetrics?: Array<{ name: string; display_name: string; data_type: string; unit?: string }> | null
  extensionSources: SelectedExtensionSource[]
  onExtensionSourcesChange: (sources: SelectedExtensionSource[]) => void
  onInsertVariable: (variable: string) => void
  formErrors: FormErrors
  // Test strip props
  testInput: string
  onTestInputChange: (v: string) => void
  testOutput: string
  testError: string
  testRunning: boolean
  onTest: () => void
  onClearTest: () => void
  tBuilder: (key: string) => string
  t: (key: string, params?: Record<string, unknown>) => string
}

function TransformWorkspace({
  jsCode,
  onCodeChange,
  templates,
  onApplyTemplate,
  deviceTypeMetrics,
  extensionSources,
  onExtensionSourcesChange,
  onInsertVariable,
  formErrors,
  testInput,
  onTestInputChange,
  testOutput,
  testError,
  testRunning,
  onTest,
  onClearTest,
  tBuilder,
  t,
}: TransformWorkspaceProps) {
  const isMobile = useIsMobile()

  // Grouped selected extension sources for badge display
  const selectedSourceBadges = useMemo(() => {
    if (!extensionSources || extensionSources.length === 0) return []
    const grouped: Record<string, { name: string; count: number }> = {}
    for (const s of extensionSources) {
      if (!grouped[s.extension_id]) {
        grouped[s.extension_id] = { name: s.extension_name, count: 0 }
      }
      grouped[s.extension_id].count++
    }
    return Object.entries(grouped).map(([id, info]) => ({ id, ...info }))
  }, [extensionSources])

  return (
    <div className="flex flex-col gap-4 h-full">
      {/* Sub-toolbar: templates dropdown + selected sources — fixed single-row height */}
      <div className="rounded-lg border border-border px-3 py-2 shrink-0 flex items-center gap-2">
        <Select
          onValueChange={(key) => {
            const tpl = templates.find((t) => t.key === key)
            if (tpl) onApplyTemplate(tpl.code)
          }}
        >
          <SelectTrigger className="h-7 w-[150px] text-xs gap-1.5 shrink-0">
            <FileCode className="h-3.5 w-3.5 text-muted-foreground" />
            <SelectValue placeholder={tBuilder('templatesLabel')} />
          </SelectTrigger>
          <SelectContent>
            {templates.map((tpl) => (
              <SelectItem key={tpl.key} value={tpl.key} className="text-xs">
                {tBuilder(tpl.nameKey)}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
        {/* Selected extension source badges */}
        {selectedSourceBadges.length > 0 && (
          <div className="flex items-center gap-1 overflow-x-auto overflow-y-hidden flex-nowrap min-w-0 ml-auto [scrollbar-width:thin]">
            {selectedSourceBadges.map(({ id, name, count }) => (
              <Badge key={id} variant="secondary" className="h-6 text-xs gap-1 shrink-0 whitespace-nowrap">
                <Puzzle className="h-3 w-3 text-accent-purple" />
                {name}
                <span className="text-muted-foreground">x{count}</span>
              </Badge>
            ))}
          </div>
        )}
      </div>

      {/* Unified panel: CSS Grid with minmax(0,1fr) row — height is container-driven,
          never inflated by rail tab content. Mobile stacks naturally. */}
      <div className="flex-1 min-h-0 rounded-lg border border-border overflow-hidden grid grid-cols-1 md:grid-cols-[1fr_288px] md:[grid-template-rows:minmax(0,1fr)]">
        <div className="min-w-0 min-h-0 flex flex-col">
          <CodeEditor
            value={jsCode}
            onChange={onCodeChange}
            height="100%"
            className="border-0 rounded-none flex-1 min-h-0 focus-within:ring-0 focus-within:ring-offset-0"
          />
          {formErrors.code && (
            <div className="px-3 pb-2 shrink-0">
              <p className="text-xs text-error">{formErrors.code}</p>
            </div>
          )}
        </div>
        {/* Variables rail — grid cell stretches to row height; content scrolls internally */}
        <div className="border-t md:border-t-0 md:border-l min-h-0 overflow-hidden">
          <div className="h-[360px] md:h-full">
            <VariablesRail
              deviceTypeMetrics={deviceTypeMetrics || undefined}
              extensionSources={extensionSources}
              onExtensionSourcesChange={onExtensionSourcesChange}
              onInsertVariable={onInsertVariable}
              tBuilder={tBuilder}
              t={t}
            />
          </div>
        </div>
      </div>

      {/* Test strip */}
      <TestStrip
        jsCode={jsCode}
        testInput={testInput}
        onTestInputChange={onTestInputChange}
        testOutput={testOutput}
        testError={testError}
        testRunning={testRunning}
        onTest={onTest}
        onClearTest={onClearTest}
        deviceTypeMetrics={deviceTypeMetrics || undefined}
        extensionSources={extensionSources}
        tBuilder={tBuilder}
      />
    </div>
  )
}

// ============================================================================
// Main Component
// ============================================================================

export function TransformBuilder({
  open,
  onOpenChange,
  transform,
  devices,
  deviceTypes: deviceTypesProp,
  onSave,
}: TransformBuilderProps) {
  const { t } = useTranslation(['automation', 'common'])
  const tBuilder = (key: string) => t(`automation:transformBuilder.${key}`)
  const isEditMode = !!transform

  // Form data
  const [name, setName] = useState('')
  const [description, setDescription] = useState('')
  const [enabled, setEnabled] = useState(true)
  const [scopeType, setScopeType] = useState<ScopeType>('global')
  const [scopeValue, setScopeValue] = useState('')
  const [outputPrefix, setOutputPrefix] = useState('transform')
  const [jsCode, setJsCode] = useState('')

  // Extension data sources (NEW: selected extension sources for this transform)
  const [selectedExtensionSources, setSelectedExtensionSources] = useState<SelectedExtensionSource[]>([])

  // Test state
  const [testInput, setTestInput] = useState('')
  const [testOutput, setTestOutput] = useState('')
  const [testError, setTestError] = useState('')
  const [testRunning, setTestRunning] = useState(false)

  // Device type metrics state
  const [deviceTypeMetrics, setDeviceTypeMetrics] = useState<Array<{ name: string; display_name: string; data_type: string; unit?: string }> | null>(null)

  // Validation state
  const [formErrors, setFormErrors] = useState<FormErrors>({})

  // Get all device types (use prop if provided, otherwise extract from devices)
  const deviceTypes = useMemo(() => {
    if (deviceTypesProp && deviceTypesProp.length > 0) {
      return deviceTypesProp.map(dt => dt.device_type)
    }
    return Array.from(new Set(devices.map((d) => d.device_type).filter((dt): dt is string => Boolean(dt))))
  }, [deviceTypesProp, devices])

  // Build scope options
  const scopeOptions: Array<{ value: string; label: string }> = useMemo(() => {
    if (scopeType === 'device_type') {
      return deviceTypes.map(dt => ({ value: dt, label: dt }))
    }
    if (scopeType === 'device') {
      return devices.map(d => ({ value: d.id, label: d.name }))
    }
    return []
  }, [scopeType, deviceTypes, devices])

  // Auto-select first scope option when scopeType changes (only for NEW transforms)
  // When editing, scopeValue is set from the existing transform and should not be overwritten.
  useEffect(() => {
    if (transform) return // Don't auto-select when editing existing transform
    if (scopeType !== 'global' && scopeOptions.length > 0) {
      setScopeValue(scopeOptions[0].value)
    } else if (scopeType === 'global') {
      setScopeValue('')
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [scopeType])

  // Fetch device type metrics for the selected scope
  useEffect(() => {
    const fetchMetrics = async () => {
      if (scopeType === 'device_type' && scopeValue) {
        try {
          const deviceTypeData = await api.getDeviceType(scopeValue)
          setDeviceTypeMetrics(deviceTypeData.metrics || null)
        } catch {
          setDeviceTypeMetrics(null)
        }
      } else if (scopeType === 'device' && scopeValue) {
        try {
          const device = await api.getDevice(scopeValue)
          if (device.device_type) {
            try {
              const deviceTypeData = await api.getDeviceType(device.device_type)
              setDeviceTypeMetrics(deviceTypeData.metrics || null)
            } catch {
              setDeviceTypeMetrics(null)
            }
          } else {
            setDeviceTypeMetrics(null)
          }
        } catch {
          setDeviceTypeMetrics(null)
        }
      } else {
        setDeviceTypeMetrics(null)
      }
    }

    const timeoutId = setTimeout(fetchMetrics, 300)
    return () => clearTimeout(timeoutId)
  }, [scopeType, scopeValue])

  // Reset form helper
  const resetForm = useCallback(() => {
    setName('')
    setDescription('')
    setEnabled(true)
    setScopeType('global')
    setScopeValue('')
    setOutputPrefix('transform')
    setJsCode('')
    setSelectedExtensionSources([])
    setTestInput('')
    setTestOutput('')
    setTestError('')
    setDeviceTypeMetrics(null)
    setFormErrors({})
  }, [])

  // Reset when dialog opens or transform changes
  useEffect(() => {
    if (open) {
      if (transform) {
        setName(transform.name)
        setDescription(transform.description || '')
        setEnabled(transform.enabled)
        setOutputPrefix(transform.output_prefix ?? 'transform')
        setJsCode(transform.js_code || '')

        // Handle scope format
        if (transform.scope === 'global') {
          setScopeType('global')
          setScopeValue('')
        } else if (typeof transform.scope === 'object') {
          if ('device_type' in transform.scope) {
            setScopeType('device_type')
            setScopeValue(transform.scope.device_type || '')
          } else if ('device' in transform.scope) {
            setScopeType('device')
            setScopeValue(transform.scope.device || '')
          }
        }

        // Reset extension sources (will be reloaded if we add storage for them)
        setSelectedExtensionSources([])
      } else {
        resetForm()
      }
    }
  }, [open, transform, resetForm])

  // Apply template
  const handleApplyTemplate = useCallback((templateCode: string) => {
    setJsCode(templateCode)
  }, [])

  // Insert variable into code
  const handleInsertVariable = useCallback((variable: string) => {
    // Try CodeMirror first
    const cmEditor = document.querySelector('.cm-content')?.closest('.cm-editor')
    if (cmEditor) {
      const text = jsCode
      // For CodeMirror, just append to the end for now
      // A more sophisticated implementation would use the CM API
      const insertion = text.length > 0 && !text.endsWith(' ') && !text.endsWith('\n') ? ` ${variable}` : variable
      setJsCode(text + insertion)
    } else {
      // Fallback: append to code
      setJsCode(jsCode + (jsCode.length > 0 && !jsCode.endsWith(' ') && !jsCode.endsWith('\n') ? ' ' : '') + variable)
    }
  }, [jsCode])

  // Test code using backend API (supports extension invocation)
  const handleTestCode = useCallback(async () => {
    if (!jsCode.trim()) return

    setTestRunning(true)
    setTestOutput('')
    setTestError('')

    try {
      const inputData: Record<string, unknown> = testInput.trim()
        ? JSON.parse(testInput)
        : { temperature: 25, humidity: 60 }

      // Add mock extension data if sources are selected
      if (selectedExtensionSources.length > 0) {
        const extensions: Record<string, Record<string, Record<string, unknown>>> = {}
        inputData.extensions = extensions
        for (const source of selectedExtensionSources) {
          if (!extensions[source.extension_id]) {
            extensions[source.extension_id] = {}
          }
          if (!extensions[source.extension_id][source.command]) {
            extensions[source.extension_id][source.command] = {}
          }
          // Add mock value based on data type
          switch (source.data_type) {
            case 'integer':
              extensions[source.extension_id][source.command][source.field] = 42
              break
            case 'number':
              extensions[source.extension_id][source.command][source.field] = 42.5
              break
            case 'boolean':
              extensions[source.extension_id][source.command][source.field] = true
              break
            case 'string':
              extensions[source.extension_id][source.command][source.field] = 'sample'
              break
            default:
              extensions[source.extension_id][source.command][source.field] = null
          }
        }
      }

      // Call backend API to test the code
      const result = await api.testTransformCode({
        code: jsCode,
        input_data: inputData,
        output_prefix: outputPrefix,
      })

      if (result.success) {
        // Display output with prefix for clarity
        setTestOutput(JSON.stringify(result.output_with_prefix || result.output, null, 2))
      } else {
        setTestError(result.error || 'Unknown error')
      }
    } catch (err) {
      setTestError(err instanceof Error ? err.message : String(err))
    } finally {
      setTestRunning(false)
    }
  }, [jsCode, testInput, outputPrefix, selectedExtensionSources])

  // Save
  const handleSave = useCallback(() => {
    if (!name.trim()) return

    const scope: TransformScope = (() => {
      switch (scopeType) {
        case 'global': return 'global' as const
        case 'device_type': return { device_type: scopeValue }
        case 'device': return { device: scopeValue }
      }
    })()

    // Save extension sources as a JSON string in description for now
    // (In future, we may add a dedicated field)
    let finalDescription = description
    if (selectedExtensionSources.length > 0) {
      const sourcesInfo = selectedExtensionSources.map(s => ({
        ext: s.extension_id,
        cmd: s.command,
        field: s.field,
      }))
      finalDescription = finalDescription
        ? `${finalDescription}\n\nExtension sources: ${JSON.stringify(sourcesInfo)}`
        : `Extension sources: ${JSON.stringify(sourcesInfo)}`
    }

    onSave({
      name,
      description: finalDescription,
      enabled,
      scope,
      js_code: jsCode,
      output_prefix: outputPrefix,
      complexity: jsCode.split('\n').length > 10 ? 3 : 2,
    })
  }, [name, description, enabled, scopeType, scopeValue, jsCode, outputPrefix, selectedExtensionSources, onSave])

  // --- Config rail content ---
  const config = (
    <div className="space-y-4">
      {/* Name */}
      <Field>
        <FieldLabel>
          {tBuilder('name')} <span className="text-error">*</span>
        </FieldLabel>
        <Input
          value={name}
          onChange={e => setName(e.target.value)}
          placeholder={tBuilder('transformNamePlaceholder')}
          aria-invalid={!!formErrors.name}
        />
        {formErrors.name && <FieldMessage>{formErrors.name}</FieldMessage>}
      </Field>

      {/* Description */}
      <Field>
        <FieldLabel>{tBuilder('description')}</FieldLabel>
        <Textarea
          value={description}
          onChange={e => setDescription(e.target.value)}
          placeholder={tBuilder('descriptionPlaceholder')}
          className="resize-none"
          rows={2}
        />
      </Field>

      {/* Scope */}
      <Field>
        <FieldLabel>{tBuilder('scopeLabel')}</FieldLabel>
        <Select value={scopeType} onValueChange={(v: string) => setScopeType(v as ScopeType)}>
          <SelectTrigger>
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value="global">{tBuilder('scope.global')}</SelectItem>
            <SelectItem value="device_type">{tBuilder('scope.deviceType')}</SelectItem>
            <SelectItem value="device">{tBuilder('scope.device')}</SelectItem>
          </SelectContent>
        </Select>
      </Field>

      {scopeType !== 'global' && (
        <Field>
          <FieldLabel>
            {scopeType === 'device_type' ? tBuilder('scope.deviceType') : tBuilder('scope.device')}
          </FieldLabel>
          <Select value={scopeValue} onValueChange={setScopeValue}>
            <SelectTrigger aria-invalid={!!formErrors.scopeValue}>
              <SelectValue placeholder={tBuilder('selectScope')} />
            </SelectTrigger>
            <SelectContent>
              {scopeOptions.map(opt => (
                <SelectItem key={opt.value} value={opt.value}>
                  {opt.label}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
          {formErrors.scopeValue && <FieldMessage>{formErrors.scopeValue}</FieldMessage>}
        </Field>
      )}

      {/* Output prefix */}
      <Field>
        <FieldLabel>{tBuilder('outputPrefix')}</FieldLabel>
        <Input
          value={outputPrefix}
          onChange={e => setOutputPrefix(e.target.value)}
          placeholder={tBuilder('outputPrefixPlaceholder')}
          className={cn(formErrors.outputPrefix && "border-error")}
        />
        {formErrors.outputPrefix && (
          <p className="text-xs text-error">{formErrors.outputPrefix}</p>
        )}
        <FieldDescription>{tBuilder('outputPrefixHint')}</FieldDescription>
      </Field>

      {/* Enabled */}
      <div className="flex items-center gap-3">
        <Switch
          id="transform-enabled"
          checked={enabled}
          onCheckedChange={(checked) => setEnabled(!!checked)}
        />
        <Label htmlFor="transform-enabled" className="text-sm font-medium cursor-pointer">
          {tBuilder('enabled')}
        </Label>
      </div>
    </div>
  )

  // --- Status indicator (enable dot) ---
  const statusIndicator = (
    <div className="flex items-center gap-2">
      <span
        className={cn(
          "inline-block w-2 h-2 rounded-full",
          enabled ? "bg-success" : "bg-muted-foreground"
        )}
      />
      <span className="text-xs text-muted-foreground">
        {enabled ? tBuilder('enabled') : tBuilder('disabled')}
      </span>
    </div>
  )

  // --- Footer ---
  const footer = (
    <>
      <div />
      <div className="flex items-center gap-2">
        <Button
          size="sm"
          onClick={handleSave}
          disabled={!name.trim()}
        >
          {tBuilder('save')}
        </Button>
      </div>
    </>
  )

  return (
    <BuilderShell
      open={open}
      onOpenChange={onOpenChange}
      accent="emerald"
      title={isEditMode ? tBuilder('editTitle') : tBuilder('title')}
      subtitle={tBuilder('desc')}
      icon={<Code className="h-5 w-5" />}
      statusIndicator={statusIndicator}
      config={config}
      workspace={
        <TransformWorkspace
          jsCode={jsCode}
          onCodeChange={setJsCode}
          templates={CODE_TEMPLATES}
          onApplyTemplate={handleApplyTemplate}
          deviceTypeMetrics={deviceTypeMetrics}
          extensionSources={selectedExtensionSources}
          onExtensionSourcesChange={setSelectedExtensionSources}
          onInsertVariable={handleInsertVariable}
          formErrors={formErrors}
          testInput={testInput}
          onTestInputChange={setTestInput}
          testOutput={testOutput}
          testError={testError}
          testRunning={testRunning}
          onTest={handleTestCode}
          onClearTest={() => { setTestOutput(''); setTestError('') }}
          tBuilder={tBuilder}
          t={t}
        />
      }
      footer={footer}
      mobileConfigLabel={tBuilder('basicInfo')}
    />
  )
}
