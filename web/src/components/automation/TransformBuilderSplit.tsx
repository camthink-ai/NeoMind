import { useState, useEffect, useCallback, useMemo } from 'react'
import { useTranslation } from 'react-i18next'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Textarea } from '@/components/ui/textarea'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { Badge } from '@/components/ui/badge'
import { ScrollArea } from '@/components/ui/scroll-area'
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogDescription,
} from '@/components/ui/dialog'
import { Code, Loader2, Play, Database, Save, FlaskConical } from 'lucide-react'
import { api } from '@/lib/api'
import type { TransformAutomation, TransformScope } from '@/types'
import { cn } from '@/lib/utils'

interface TransformBuilderProps {
  open: boolean
  onOpenChange: (open: boolean) => void
  transform?: TransformAutomation | null
  devices: Array<{ id: string; name: string; device_type?: string }>
  onSave: (data: Partial<TransformAutomation>) => void
}

type ScopeType = 'global' | 'device_type' | 'device'

// Code templates for common data transformations
const CODE_TEMPLATES = [
  {
    key: 'temperature',
    code: '// Input: input.temp_c (Celsius)\nreturn {\n  temp_f: (input.temp_c || input.temperature || 0) * 9 / 5 + 32\n};',
  },
  {
    key: 'batteryStatus',
    code: '// Input: input.battery (0-100)\nconst battery = input.battery || input.batt || 0;\nreturn {\n  battery_percent: Math.min(100, Math.max(0, battery)),\n  battery_status: battery > 80 ? \'good\' : battery > 20 ? \'medium\' : \'low\'\n};',
  },
  {
    key: 'hexParse',
    code: '// Input: input.hex or input.data (hex string)\nconst hex = input.hex || input.data || \'\';\nconst str = hex.match(/.{1,2}/g)?.map(b => String.fromCharCode(parseInt(b, 16))).join(\"\") || \'\';\ntry {\n  return JSON.parse(str);\n} catch {\n  return { parsed: str };\n}',
  },
  {
    key: 'dataAggregate',
    code: '// Input: input.values (array of numbers)\nconst readings = input.values || input.readings || [];\nconst avg = readings.reduce((sum, v) => sum + (v || 0), 0) / readings.length;\nreturn {\n  average: parseFloat(avg.toFixed(2)),\n  count: readings.length,\n  min: Math.min(...readings),\n  max: Math.max(...readings)\n};',
  },
  {
    key: 'addMetrics',
    code: '// Input: input.temp, input.humidity, etc.\nreturn {\n  is_normal: (input.value || input.val || 0) > 0,\n  status_level: (input.confidence || input.conf || 1) > 0.8 ? 1 : 0,\n  event_type: input.type || \'unknown\',\n  processed_at: Date.now()\n};',
  },
  {
    key: 'statusCheck',
    code: '// Input: input.value (sensor reading)\nconst value = input.value || input.val || 0;\nreturn {\n  status: value > 100 ? \'critical\' : value > 80 ? \'warning\' : \'normal\',\n  is_alert: value > 100,\n  severity: value > 100 ? 3 : value > 80 ? 2 : 1\n};',
  },
  {
    key: 'passThrough',
    code: '// Pass through all input data unchanged\nreturn input;',
  },
]

function MetricsPreviewPanel({
  scopeType,
  scopeValue,
  deviceTypeMetrics,
}: {
  scopeType: ScopeType
  scopeValue: string
  deviceTypeMetrics?: Array<{ name: string; display_name: string; data_type: string; unit?: string }>
}) {
  const { t } = useTranslation(['automation'])

  const getScopeLabel = () => {
    switch (scopeType) {
      case 'global': return t('automation:scopes.global')
      case 'device_type': return `${t('automation:deviceType')}: ${scopeValue || '-'}`
      case 'device': return `${t('common:device')}: ${scopeValue || '-'}`
    }
  }

  const getTypeColor = (type: string) => {
    switch (type) {
      case 'number': case 'integer': case 'float': return 'text-blue-500'
      case 'string': return 'text-green-500'
      case 'boolean': return 'text-purple-500'
      case 'object': return 'text-orange-500'
      case 'array': return 'text-cyan-500'
      case 'binary': return 'text-yellow-500'
      default: return 'text-gray-500'
    }
  }

  const getTypeIcon = (type: string) => {
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

  // Convert device type metrics to MetricPreview format
  const deviceMetricsPreview = useMemo(() => {
    if (!deviceTypeMetrics) return []
    return deviceTypeMetrics.map(m => ({
      name: m.name,
      displayName: m.display_name,
      type: m.data_type === 'integer' || m.data_type === 'float' ? 'number' : m.data_type as any,
      unit: m.unit
    }))
  }, [deviceTypeMetrics])

  return (
    <div className="h-full flex flex-col">
      {/* Header */}
      <div className="px-4 py-3 border-b bg-muted/20 flex-shrink-0">
        <div className="flex items-center gap-2">
          <Database className="h-4 w-4 text-blue-500" />
          <span className="font-semibold text-sm">{t('automation:testData')}</span>
        </div>
      </div>

      <div className="flex-1 overflow-auto p-3 space-y-3">
        {/* Scope info */}
        <div className="p-2 bg-muted/50 rounded text-xs">
          <span className="text-muted-foreground">{t('automation:scope')}: </span>
          <span className="font-medium">{getScopeLabel()}</span>
        </div>

        {/* Device Type Metrics (when device_type or device is selected) */}
        {(scopeType === 'device' || scopeType === 'device_type') && deviceMetricsPreview.length > 0 && (
          <div className="flex-1 min-h-0">
            <div className="text-xs font-medium text-muted-foreground mb-2 flex items-center gap-1">
              <Database className="h-3 w-3" />
              {t('automation:transformBuilder.availableVars')}
            </div>
            <ScrollArea className="h-full">
              <div className="space-y-1.5 pr-2">
                {deviceMetricsPreview.map((metric, idx) => (
                  <div
                    key={idx}
                    className="flex items-center justify-between p-2 bg-background border rounded hover:bg-muted/50 transition-colors"
                  >
                    <div className="flex items-center gap-2 min-w-0">
                      <code className="text-xs font-mono text-blue-600 dark:text-blue-400 truncate">
                        {metric.name}
                      </code>
                      {metric.displayName && metric.displayName !== metric.name && (
                        <span className="text-xs text-muted-foreground truncate">{metric.displayName}</span>
                      )}
                    </div>
                    <div className="flex items-center gap-1.5 flex-shrink-0">
                      {metric.unit && (
                        <span className="text-xs text-muted-foreground">{metric.unit}</span>
                      )}
                      <Badge variant="outline" className={cn('text-xs h-5 px-1.5', getTypeColor(metric.type))}>
                        {getTypeIcon(metric.type)}
                      </Badge>
                    </div>
                  </div>
                ))}
              </div>
            </ScrollArea>
          </div>
        )}

        {/* Hint for global scope */}
        {scopeType === 'global' && (
          <div className="flex-1 flex items-center justify-center text-center p-4">
            <div className="text-sm text-muted-foreground">
              <Database className="h-8 w-8 mx-auto mb-2 opacity-50" />
              {t('automation:transformBuilder.scopes.global')}<br />
              {t('automation:transformBuilder.accessVia')} <code className="text-xs bg-muted px-1 rounded">input</code> {t('automation:transformBuilder.accessRawData')}
            </div>
          </div>
        )}
      </div>
    </div>
  )
}

export function TransformBuilder({
  open,
  onOpenChange,
  transform,
  devices,
  onSave,
}: TransformBuilderProps) {
  const { t } = useTranslation(['automation', 'common'])

  // Form state
  const [name, setName] = useState('')
  const [description, setDescription] = useState('')
  const [enabled, setEnabled] = useState(true)
  const [scopeType, setScopeType] = useState<ScopeType>('global')
  const [scopeValue, setScopeValue] = useState('')
  const [outputPrefix, setOutputPrefix] = useState('') // Empty by default = no prefix

  // Code state
  const [jsCode, setJsCode] = useState('')

  // Test state
  const [testInput, setTestInput] = useState('')
  const [testOutput, setTestOutput] = useState('')
  const [testError, setTestError] = useState('')
  const [testRunning, setTestRunning] = useState(false)

  // Device type metrics state
  const [deviceTypeMetrics, setDeviceTypeMetrics] = useState<Array<{ name: string; display_name: string; data_type: string; unit?: string }> | null>(null)

  // Get all device types
  const deviceTypes = useMemo(() => {
    return Array.from(new Set(devices.map((d) => d.device_type).filter(Boolean)))
  }, [devices])

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

  // Reset form when transform changes
  useEffect(() => {
    if (open && transform) {
      setName(transform.name)
      setDescription(transform.description || '')
      setEnabled(transform.enabled)
      setOutputPrefix(transform.output_prefix || 'transform')
      setJsCode(transform.js_code || '')

      // Handle new scope format: 'global' | { device_type: string } | { device: string }
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
    } else if (open) {
      resetForm()
    }
  }, [transform, open])

  const resetForm = useCallback(() => {
    setName('')
    setDescription('')
    setEnabled(true)
    setScopeType('global')
    setScopeValue('')
    setOutputPrefix('')
    setJsCode('')
    setTestInput('')
    setTestOutput('')
    setTestError('')
    setDeviceTypeMetrics(null)
  }, [])

  // Apply template
  const handleApplyTemplate = useCallback((templateCode: string) => {
    setJsCode(templateCode)
  }, [])

  // Test code - shows the actual result as returned by the code
  const handleTestCode = useCallback(async () => {
    if (!jsCode.trim()) return

    setTestRunning(true)
    setTestOutput('')
    setTestError('')

    try {
      const inputData = testInput.trim()
        ? JSON.parse(testInput)
        : { temp: 25, humidity: 60 }

      const fn = new Function('input', jsCode)
      const result = fn(inputData)

      if (typeof result === 'object' && result !== null) {
        setTestOutput(JSON.stringify(result, null, 2))
      } else {
        setTestOutput(String(result))
      }
    } catch (err) {
      setTestError(err instanceof Error ? err.message : String(err))
    } finally {
      setTestRunning(false)
    }
  }, [jsCode, testInput])

  // Save
  const handleSave = useCallback(() => {
    if (!name.trim()) return

    const scope: TransformScope = (() => {
      switch (scopeType) {
        case 'global':
          return 'global' as const
        case 'device_type':
          return { device_type: scopeValue }
        case 'device':
          return { device: scopeValue }
      }
    })()

    onSave({
      name,
      description,
      enabled,
      scope,
      js_code: jsCode,
      output_prefix: outputPrefix,
      complexity: jsCode.split('\n').length > 10 ? 3 : 2,
    })
  }, [name, description, enabled, scopeType, scopeValue, jsCode, outputPrefix, onSave])

  // Validation
  const isValid = Boolean(name.trim() && jsCode.trim())

  // Get selected scope display name (for preview panel)
  const getScopeDisplayName = () => {
    if (scopeType === 'device_type') return `${t('automation:deviceType')}: ${scopeValue || '-'}`
    if (scopeType === 'device') {
      const device = devices.find(d => d.id === scopeValue)
      return `${t('common:device')}: ${device?.name || scopeValue || '-'}`
    }
    return t('automation:scopes.global')
  }

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-5xl h-[85vh] p-0 flex flex-col">
        <DialogHeader className="px-6 py-4 border-b">
          <div className="flex items-center justify-between">
            <div className="flex items-center gap-3">
              <Code className="h-5 w-5 text-blue-500" />
              <div>
                <DialogTitle className="text-lg font-semibold">
                  {transform ? t('automation:transformBuilder.editTitle') : t('automation:transformBuilder.title')}
                </DialogTitle>
                <DialogDescription className="text-sm">
                  {t('automation:transformBuilder.description')}
                </DialogDescription>
              </div>
            </div>
          </div>
        </DialogHeader>

        {/* Form Section */}
        <div className="border-b px-6 py-4 bg-muted/20 flex-shrink-0">
          {/* Row 1: Name, Description */}
          <div className="grid grid-cols-2 gap-4 mb-4">
            <div>
              <Label htmlFor="transform-name" className="text-xs font-medium">{t('automation:transformBuilder.name')} <span className="text-destructive">*</span></Label>
              <Input
                id="transform-name"
                value={name}
                onChange={e => setName(e.target.value)}
                placeholder={t('automation:transformBuilder.namePlaceholder')}
                className="mt-1.5 h-9"
              />
            </div>
            <div>
              <Label htmlFor="transform-desc" className="text-xs font-medium">{t('automation:description')}</Label>
              <Input
                id="transform-desc"
                value={description}
                onChange={e => setDescription(e.target.value)}
                placeholder={t('automation:transformDescriptionPlaceholder')}
                className="mt-1.5 h-9"
              />
            </div>
          </div>

          {/* Row 2: Scope Type, Scope Value */}
          <div className="grid grid-cols-2 gap-4">
            <div>
              <Label className="text-xs font-medium">{t('automation:scope')}</Label>
              <Select value={scopeType} onValueChange={(v: ScopeType) => setScopeType(v)}>
                <SelectTrigger className="mt-1.5 h-9">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="global">{t('automation:scopes.global')}</SelectItem>
                  <SelectItem value="device_type">{t('automation:scopes.deviceType')}</SelectItem>
                  <SelectItem value="device">{t('automation:scopes.device')}</SelectItem>
                </SelectContent>
              </Select>
            </div>

            {scopeType === 'device_type' && (
              <div>
                <Label className="text-xs font-medium">{t('automation:deviceType')}</Label>
                <Select value={scopeValue} onValueChange={setScopeValue}>
                  <SelectTrigger className="mt-1.5 h-9">
                    <SelectValue placeholder={t('automation:selectPlaceholder')} />
                  </SelectTrigger>
                  <SelectContent>
                    {deviceTypes.map(dt => (
                      <SelectItem key={dt} value={dt || ''}>{dt}</SelectItem>
                    ))}
                  </SelectContent>
                </Select>
              </div>
            )}
            {scopeType === 'device' && (
              <div>
                <Label className="text-xs font-medium">{t('common:device')}</Label>
                <Select value={scopeValue} onValueChange={setScopeValue}>
                  <SelectTrigger className="mt-1.5 h-9">
                    <SelectValue placeholder={t('automation:selectPlaceholder')} />
                  </SelectTrigger>
                  <SelectContent>
                    {devices.map(d => (
                      <SelectItem key={d.id} value={d.id}>{d.name}</SelectItem>
                    ))}
                  </SelectContent>
                </Select>
              </div>
            )}
            {scopeType === 'global' && (
              <div>
                <Label className="text-xs font-medium">{t('automation:transformBuilder.scopeLabel')}</Label>
                <div className="mt-1.5 h-9 flex items-center text-sm text-muted-foreground px-3 bg-background border rounded-md">
                  {t('automation:transformBuilder.scopes.global')}
                </div>
              </div>
            )}
          </div>
        </div>

        {/* Main Content - Code Editor + Input Preview */}
        <div className="flex-1 min-h-0 flex">
          {/* Left - Code Editor */}
          <div className="flex-1 flex flex-col min-w-0">
            {/* Templates */}
            <div className="px-6 py-3 border-b bg-muted/20 flex-shrink-0">
              <Label className="text-xs font-medium text-muted-foreground mb-2 block">{t('automation:transformBuilder.availableVars')}</Label>
              <div className="flex flex-wrap gap-2">
                {CODE_TEMPLATES.map((tpl, i) => (
                  <Button
                    key={i}
                    variant="outline"
                    size="sm"
                    type="button"
                    onClick={() => handleApplyTemplate(tpl.code)}
                    className="h-8 text-xs"
                  >
                    {t(`automation:transformBuilder.templates.${tpl.key}` as any)}
                  </Button>
                ))}
              </div>
            </div>

            {/* Code Editor */}
            <div className="flex-1 flex flex-col min-h-0">
              <div className="px-6 py-3 border-b bg-muted/30 flex items-center justify-between flex-shrink-0">
                <Label className="text-sm font-medium">{t('automation:transformBuilder.transformCode')}</Label>
                <span className="text-xs text-muted-foreground">
                  {t('automation:transformBuilder.transformCodeDesc')}
                </span>
              </div>
              <Textarea
                value={jsCode}
                onChange={e => setJsCode(e.target.value)}
                placeholder={t('automation:transformBuilder.transformCodeDesc')}
                className="flex-1 resize-none font-mono text-sm rounded-none border-r focus-visible:ring-0 p-4"
                spellCheck={false}
              />
            </div>
          </div>

          {/* Right - Input Data Preview */}
          <div className="w-80 border-l flex flex-col bg-muted/10">
            <div className="flex-1 overflow-auto">
              <MetricsPreviewPanel
                scopeType={scopeType}
                scopeValue={getScopeDisplayName()}
                deviceTypeMetrics={deviceTypeMetrics || undefined}
              />
            </div>
          </div>
        </div>

        {/* Bottom Row: Test (Left) + Output (Right) */}
        <div className="h-52 border-t flex">
          {/* Test Section */}
          <div className="w-1/2 border-r p-4 flex flex-col">
            <Label className="text-xs font-medium mb-2 flex items-center gap-2">
              <Play className="h-3 w-3" />
              {t('automation:test')}
            </Label>
            <div className="flex-1 flex flex-col min-h-0">
              <Textarea
                value={testInput}
                onChange={e => setTestInput(e.target.value)}
                placeholder='{"temp": 25, "humidity": 60}'
                className="flex-1 font-mono text-xs resize-none mb-2 bg-muted/30"
              />
              <div className="flex gap-2 flex-shrink-0">
                <Button
                  size="sm"
                  onClick={handleTestCode}
                  disabled={!jsCode || testRunning}
                  className="h-8"
                >
                  {testRunning ? <Loader2 className="h-3 w-3 animate-spin" /> : <Play className="h-3 w-3 mr-1" />}
                  {t('automation:transformBuilder.run')}
                </Button>
                {(scopeType === 'device_type' || scopeType === 'device') && deviceTypeMetrics && deviceTypeMetrics.length > 0 && (
                  <Button
                    size="sm"
                    variant="outline"
                    onClick={() => {
                      const mockData: Record<string, unknown> = {}
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
                      setTestInput(JSON.stringify(mockData, null, 2))
                    }}
                    className="h-8"
                  >
                    <FlaskConical className="h-3 w-3 mr-1" />
                    {t('automation:transformBuilder.mockData')}
                  </Button>
                )}
                {(testOutput || testError) && (
                  <Button
                    size="sm"
                    variant="ghost"
                    onClick={() => { setTestOutput(''); setTestError('') }}
                    className="h-8"
                  >
                    {t('automation:transformBuilder.clear')}
                  </Button>
                )}
              </div>
            </div>
          </div>

          {/* Test Output */}
          <div className="w-1/2 p-4 flex flex-col">
            <Label className="text-xs font-medium mb-2 flex items-center gap-2">
              <Database className="h-3 w-3" />
              {t('automation:transformBuilder.outputData')}
            </Label>
            <div className="flex-1 min-h-0 overflow-auto rounded-md bg-muted/30 p-2">
              {testError && (
                <div className="p-2 bg-destructive/10 border border-destructive/20 rounded text-xs text-destructive font-mono">
                  {testError}
                </div>
              )}
              {testOutput && !testError && (
                <pre className="text-xs font-mono text-muted-foreground">
                  {testOutput}
                </pre>
              )}
              {!testOutput && !testError && (
                <div className="text-xs text-muted-foreground text-center py-8">
                  {t('automation:transformBuilder.clickRunToSeeOutput')}
                </div>
              )}
            </div>
          </div>
        </div>

        {/* Footer */}
        <div className="flex items-center justify-end px-6 py-3 border-t bg-muted/20 flex-shrink-0">
          <div className="flex gap-2">
            <Button variant="outline" onClick={() => onOpenChange(false)}>
              {t('common:cancel')}
            </Button>
            <Button onClick={handleSave} disabled={!isValid}>
              <Save className="h-4 w-4 mr-1" />
              {t('common:save')}
            </Button>
          </div>
        </div>
      </DialogContent>
    </Dialog>
  )
}
