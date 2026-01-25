/**
 * TransformBuilderSplit Component
 *
 * Step-by-step dialog for creating/editing data transforms.
 * Following the same pattern as DeviceTypeDialog.
 *
 * @module automation
 */

import { useState, useEffect, useCallback, useMemo } from 'react'
import { useTranslation } from 'react-i18next'
import {
  Dialog,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Textarea } from '@/components/ui/textarea'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { Badge } from '@/components/ui/badge'
import {
  Code,
  Loader2,
  Play,
  Database,
  FlaskConical,
  Settings,
  ChevronLeft,
  ChevronRight,
  Check,
  X,
} from 'lucide-react'
import { api } from '@/lib/api'
import type { TransformAutomation, TransformScope } from '@/types'
import { cn } from '@/lib/utils'

// ============================================================================
// Types
// ============================================================================

interface TransformBuilderProps {
  open: boolean
  onOpenChange: (open: boolean) => void
  transform?: TransformAutomation | null
  devices: Array<{ id: string; name: string; device_type?: string }>
  onSave: (data: Partial<TransformAutomation>) => void
}

type Step = 'basic' | 'code' | 'test'
type ScopeType = 'global' | 'device_type' | 'device'

interface FormErrors {
  name?: string
  code?: string
  outputPrefix?: string
  scopeValue?: string
}

// Code templates for common data transformations
const CODE_TEMPLATES = [
  {
    key: 'temperature',
    name: '温度转换',
    code: '// Input: input.temp_c (Celsius)\nreturn {\n  temp_f: (input.temp_c || input.temperature || 0) * 9 / 5 + 32\n};',
  },
  {
    key: 'batteryStatus',
    name: '电池状态',
    code: '// Input: input.battery (0-100)\nconst battery = input.battery || input.batt || 0;\nreturn {\n  battery_percent: Math.min(100, Math.max(0, battery)),\n  battery_status: battery > 80 ? \'good\' : battery > 20 ? \'medium\' : \'low\'\n};',
  },
  {
    key: 'hexParse',
    name: 'Hex 解析',
    code: '// Input: input.hex or input.data (hex string)\nconst hex = input.hex || input.data || \'\';\nconst str = hex.match(/.{1,2}/g)?.map(b => String.fromCharCode(parseInt(b, 16))).join(\"\") || \'\';\ntry {\n  return JSON.parse(str);\n} catch {\n  return { parsed: str };\n}',
  },
  {
    key: 'dataAggregate',
    name: '数据聚合',
    code: '// Input: input.values (array of numbers)\nconst readings = input.values || input.readings || [];\nconst avg = readings.reduce((sum, v) => sum + (v || 0), 0) / readings.length;\nreturn {\n  average: parseFloat(avg.toFixed(2)),\n  count: readings.length,\n  min: Math.min(...readings),\n  max: Math.max(...readings)\n};',
  },
  {
    key: 'addMetrics',
    name: '添加指标',
    code: '// Input: input.temp, input.humidity, etc.\nreturn {\n  is_normal: (input.value || input.val || 0) > 0,\n  status_level: (input.confidence || input.conf || 1) > 0.8 ? 1 : 0,\n  event_type: input.type || \'unknown\',\n  processed_at: Date.now()\n};',
  },
  {
    key: 'statusCheck',
    name: '状态检查',
    code: '// Input: input.value (sensor reading)\nconst value = input.value || input.val || 0;\nreturn {\n  status: value > 100 ? \'critical\' : value > 80 ? \'warning\' : \'normal\',\n  is_alert: value > 100,\n  severity: value > 100 ? 3 : value > 80 ? 2 : 1\n};',
  },
  {
    key: 'passThrough',
    name: '直接透传',
    code: '// Pass through all input data unchanged\nreturn input;',
  },
]

// ============================================================================
// Metrics Preview Panel
// ============================================================================

interface MetricsPreviewPanelProps {
  scopeType: ScopeType
  scopeValue: string
  deviceTypeMetrics?: Array<{ name: string; display_name: string; data_type: string; unit?: string }>
  tBuilder: (key: string) => string
}

function MetricsPreviewPanel({
  scopeType,
  scopeValue,
  deviceTypeMetrics,
  tBuilder,
}: MetricsPreviewPanelProps) {
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

  return (
    <div className="h-full flex flex-col">
      <div className="px-4 md:px-6 py-4 border-b bg-muted/20 flex-shrink-0">
        <div className="flex items-center gap-2">
          <Database className="h-4 w-4 text-blue-500" />
          <span className="font-semibold text-sm">{tBuilder('availableVars')}</span>
        </div>
      </div>

      <div className="flex-1 overflow-auto p-3 space-y-3">
        <div className="p-2 bg-muted/50 rounded text-xs">
          <span className="text-muted-foreground">{tBuilder('scopeLabel')}: </span>
          <span className="font-medium ml-1">
            {scopeType === 'global' ? tBuilder('scope.global') :
             scopeType === 'device_type' ? `${tBuilder('scope.deviceType')}: ${scopeValue || '-'}` :
             `${tBuilder('scope.device')}: ${scopeValue || '-'}`}
          </span>
        </div>

        {(scopeType === 'device' || scopeType === 'device_type') && deviceTypeMetrics && deviceTypeMetrics.length > 0 && (
          <div className="flex-1 min-h-0">
            <div className="text-xs font-medium text-muted-foreground mb-2 flex items-center gap-1">
              <Database className="h-3 w-3" />
              {tBuilder('availableVars')}
            </div>
            <div className="space-y-1.5">
              {deviceTypeMetrics.map((metric, idx) => (
                <div
                  key={idx}
                  className="flex items-center justify-between p-2 bg-background border rounded hover:bg-muted/50 transition-colors"
                >
                  <div className="flex items-center gap-2 min-w-0">
                    <code className="text-xs font-mono text-blue-600 dark:text-blue-400 truncate">
                      {metric.name}
                    </code>
                    {metric.display_name && metric.display_name !== metric.name && (
                      <span className="text-xs text-muted-foreground truncate">{metric.display_name}</span>
                    )}
                  </div>
                  <div className="flex items-center gap-1.5 flex-shrink-0">
                    {metric.unit && (
                      <span className="text-xs text-muted-foreground">{metric.unit}</span>
                    )}
                    <Badge variant="outline" className={cn('text-xs h-5 px-1.5', getTypeColor(metric.data_type))}>
                      {getTypeIcon(metric.data_type)}
                    </Badge>
                  </div>
                </div>
              ))}
            </div>
          </div>
        )}

        {scopeType === 'global' && (
          <div className="flex-1 flex items-center justify-center text-center p-4">
            <div className="text-sm text-muted-foreground">
              <Database className="h-8 w-8 mx-auto mb-2 opacity-50" />
              {tBuilder('scopes.globalDesc')}<br />
              通过 <code className="text-xs bg-muted px-1 rounded">input</code> {tBuilder('accessRawData')}
            </div>
          </div>
        )}
      </div>
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
  onSave,
}: TransformBuilderProps) {
  const { t } = useTranslation(['automation', 'common'])
  const tBuilder = (key: string) => t(`automation:transformBuilder.${key}`)
  const isEditMode = !!transform

  // Step state
  const [currentStep, setCurrentStep] = useState<Step>('basic')
  const [completedSteps, setCompletedSteps] = useState<Set<Step>>(new Set())

  // Form data
  const [name, setName] = useState('')
  const [description, setDescription] = useState('')
  const [enabled, setEnabled] = useState(true)
  const [scopeType, setScopeType] = useState<ScopeType>('global')
  const [scopeValue, setScopeValue] = useState('')
  const [outputPrefix, setOutputPrefix] = useState('transform')  // Default value
  const [jsCode, setJsCode] = useState('')

  // Test state
  const [testInput, setTestInput] = useState('')
  const [testOutput, setTestOutput] = useState('')
  const [testError, setTestError] = useState('')
  const [testRunning, setTestRunning] = useState(false)

  // Device type metrics state
  const [deviceTypeMetrics, setDeviceTypeMetrics] = useState<Array<{ name: string; display_name: string; data_type: string; unit?: string }> | null>(null)

  // Validation state
  const [formErrors, setFormErrors] = useState<FormErrors>({})

  // Get all device types
  const deviceTypes = useMemo(() => {
    return Array.from(new Set(devices.map((d) => d.device_type).filter(Boolean)))
  }, [devices])

  // Build scope options
  const scopeOptions = useMemo(() => {
    if (scopeType === 'device_type') {
      return deviceTypes.map(dt => ({ value: dt, label: dt || '' }))
    }
    if (scopeType === 'device') {
      return devices.map(d => ({ value: d.id, label: d.name }))
    }
    return []
  }, [scopeType, deviceTypes, devices])

  // Auto-select first scope option when scopeType changes
  useEffect(() => {
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

  // Reset when dialog opens or transform changes
  useEffect(() => {
    if (open) {
      setCurrentStep('basic')
      setCompletedSteps(new Set())

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
      } else {
        resetForm()
      }
    }
  }, [open, transform])

  const resetForm = useCallback(() => {
    setName('')
    setDescription('')
    setEnabled(true)
    setScopeType('global')
    setScopeValue('')
    setOutputPrefix('transform')  // Default value
    setJsCode('')
    setTestInput('')
    setTestOutput('')
    setTestError('')
    setDeviceTypeMetrics(null)
    setFormErrors({})
  }, [])

  // Apply template
  const handleApplyTemplate = useCallback((templateCode: string) => {
    setJsCode(templateCode)
  }, [])

  // Test code
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

      // Apply output prefix to result keys
      let displayResult = result
      if (typeof result === 'object' && result !== null && !Array.isArray(result)) {
        const prefixed: Record<string, unknown> = {}
        for (const [key, value] of Object.entries(result)) {
          prefixed[`${outputPrefix}.${key}`] = value
        }
        displayResult = prefixed
      }

      if (typeof displayResult === 'object' && displayResult !== null) {
        setTestOutput(JSON.stringify(displayResult, null, 2))
      } else {
        setTestOutput(String(displayResult))
      }
    } catch (err) {
      setTestError(err instanceof Error ? err.message : String(err))
    } finally {
      setTestRunning(false)
    }
  }, [jsCode, testInput, outputPrefix])

  // Validate current step
  const validateStep = (step: Step): boolean => {
    const errors: FormErrors = {}

    if (step === 'basic') {
      if (!name.trim()) {
        errors.name = tBuilder('validationErrors.name')
      }
      // Validate scope value is selected when not global
      if (scopeType !== 'global' && !scopeValue) {
        errors.scopeValue = tBuilder('validationErrors.scopeValue')
      }
    }

    if (step === 'code') {
      if (!jsCode.trim()) {
        errors.code = tBuilder('validationErrors.code')
      }
      // Validate output prefix: only lowercase English, numbers, and underscores
      if (outputPrefix && !/^[a-z0-9_]+$/.test(outputPrefix)) {
        errors.outputPrefix = tBuilder('validationErrors.outputPrefix')
      }
    }

    setFormErrors(errors)
    return Object.keys(errors).length === 0
  }

  // Navigate to next step
  const handleNext = () => {
    if (!validateStep(currentStep)) return

    const newCompleted = new Set(completedSteps)
    newCompleted.add(currentStep)
    setCompletedSteps(newCompleted)

    const steps: Step[] = ['basic', 'code', 'test']
    const currentIndex = steps.indexOf(currentStep)
    if (currentIndex < steps.length - 1) {
      setCurrentStep(steps[currentIndex + 1])
    }
  }

  // Navigate to previous step
  const handlePrevious = () => {
    const steps: Step[] = ['basic', 'code', 'test']
    const currentIndex = steps.indexOf(currentStep)
    if (currentIndex > 0) {
      setCurrentStep(steps[currentIndex - 1])
    }
  }

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

  // Step config
  const steps: { key: Step; label: string; icon: React.ReactNode }[] = [
    { key: 'basic', label: tBuilder('steps.basic'), icon: <Settings className="h-4 w-4" /> },
    { key: 'code', label: tBuilder('steps.code'), icon: <Code className="h-4 w-4" /> },
    { key: 'test', label: tBuilder('steps.test'), icon: <FlaskConical className="h-4 w-4" /> },
  ]

  const stepIndex = steps.findIndex(s => s.key === currentStep)
  const isFirstStep = currentStep === 'basic'

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-4xl h-[90vh] max-h-[90vh] flex flex-col p-0 overflow-hidden [&>[data-radix-dialog-close]]:right-6 [&>[data-radix-dialog-close]]:top-5">
        {/* Header */}
        <DialogHeader className="px-6 pt-4 pb-4 border-b">
          <DialogTitle className="text-xl flex items-center gap-3">
            <div className="w-10 h-10 rounded-xl bg-blue-500/10 flex items-center justify-center">
              <Code className="h-5 w-5 text-blue-500" />
            </div>
            {isEditMode ? tBuilder('editTitle') : tBuilder('title')}
          </DialogTitle>
        </DialogHeader>

        {/* Step Content */}
        <div className="flex-1 overflow-y-auto px-6 py-4 space-y-4">
          {/* Step Indicator */}
          <div className="flex items-center justify-center gap-2">
            {steps.map((step, index) => {
              const isCompleted = completedSteps.has(step.key)
              const isCurrent = step.key === currentStep
              const isPast = index < stepIndex

              return (
                <div key={step.key} className="flex items-center gap-2">
                  <div
                    className={cn(
                      "w-8 h-8 rounded-full flex items-center justify-center text-sm font-medium transition-colors shrink-0",
                      isCompleted && "bg-green-500 text-white",
                      isCurrent && "bg-primary text-primary-foreground ring-4 ring-primary/20",
                      !isCompleted && !isCurrent && "bg-muted text-muted-foreground"
                    )}
                  >
                    {isCompleted ? <Check className="h-4 w-4" /> : step.icon}
                  </div>
                  <span
                    className={cn(
                      "text-xs font-medium whitespace-nowrap",
                      isCurrent ? "text-foreground" : "text-muted-foreground"
                    )}
                  >
                    {step.label}
                  </span>
                  {index < steps.length - 1 && (
                    <div
                      className={cn(
                        "w-8 h-0.5 transition-colors",
                        isPast ? "bg-primary" : "bg-muted"
                      )}
                    />
                  )}
                </div>
              )
            })}
          </div>

          {/* Step 1: Basic Info */}
          {currentStep === 'basic' && (
            <BasicInfoStep
              name={name}
              onNameChange={setName}
              description={description}
              onDescriptionChange={setDescription}
              enabled={enabled}
              onEnabledChange={setEnabled}
              scopeType={scopeType}
              onScopeTypeChange={setScopeType}
              scopeValue={scopeValue}
              onScopeValueChange={setScopeValue}
              scopeOptions={scopeOptions}
              errors={formErrors}
              t={t}
              tBuilder={tBuilder}
            />
          )}

          {/* Step 2: Code */}
          {currentStep === 'code' && (
            <CodeStep
              jsCode={jsCode}
              onCodeChange={setJsCode}
              templates={CODE_TEMPLATES}
              onApplyTemplate={handleApplyTemplate}
              scopeType={scopeType}
              scopeValue={scopeType === 'global' ? tBuilder('scope.global') :
                          scopeType === 'device_type' ? `${tBuilder('scope.deviceType')}: ${scopeValue}` :
                          devices.find(d => d.id === scopeValue)?.name || scopeValue}
              deviceTypeMetrics={deviceTypeMetrics || undefined}
              errors={formErrors}
              outputPrefix={outputPrefix}
              onOutputPrefixChange={setOutputPrefix}
              t={t}
              tBuilder={tBuilder}
            />
          )}

          {/* Step 3: Test */}
          {currentStep === 'test' && (
            <TestStep
              jsCode={jsCode}
              testInput={testInput}
              onTestInputChange={setTestInput}
              testOutput={testOutput}
              testError={testError}
              testRunning={testRunning}
              onTest={handleTestCode}
              onClearTest={() => { setTestOutput(''); setTestError('') }}
              deviceTypeMetrics={deviceTypeMetrics || undefined}
              scopeType={scopeType}
              t={t}
              tBuilder={tBuilder}
            />
          )}
        </div>

        {/* Footer Navigation */}
        <DialogFooter className="px-6 pb-4 pt-4 border-t gap-2">
          {!isFirstStep && (
            <Button variant="outline" onClick={handlePrevious}>
              <ChevronLeft className="h-4 w-4 mr-1" />
              {tBuilder('previous')}
            </Button>
          )}

          <div className="flex-1" />

          <Button onClick={currentStep === 'test' ? handleSave : handleNext}>
            {currentStep === 'test' ? tBuilder('save') : tBuilder('next')}
            <ChevronRight className="h-4 w-4 ml-1" />
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}

// ============================================================================
// Step 1: Basic Info
// ============================================================================

interface BasicInfoStepProps {
  name: string
  onNameChange: (v: string) => void
  description: string
  onDescriptionChange: (v: string) => void
  enabled: boolean
  onEnabledChange: (v: boolean) => void
  scopeType: ScopeType
  onScopeTypeChange: (v: ScopeType) => void
  scopeValue: string
  onScopeValueChange: (v: string) => void
  scopeOptions: Array<{ value: string; label: string }>
  errors: FormErrors
  t: (key: string) => string
  tBuilder: (key: string) => string
}

function BasicInfoStep({
  name,
  onNameChange,
  description,
  onDescriptionChange,
  enabled,
  onEnabledChange,
  scopeType,
  onScopeTypeChange,
  scopeValue,
  onScopeValueChange,
  scopeOptions,
  errors,
  tBuilder,
}: BasicInfoStepProps) {
  return (
    <div className="space-y-6 max-w-2xl mx-auto py-4">
      <div className="text-center mb-6">
        <h3 className="text-lg font-semibold">{tBuilder('steps.basic')}</h3>
        <p className="text-sm text-muted-foreground">{tBuilder('steps.basicDesc')}</p>
      </div>

      {/* 转换名称 */}
      <div className="space-y-2">
        <Label className="text-sm font-medium">
          {tBuilder('name')} <span className="text-destructive">*</span>
        </Label>
        <Input
          value={name}
          onChange={e => onNameChange(e.target.value)}
          placeholder={tBuilder('transformNamePlaceholder')}
          className={cn(errors.name && "border-destructive")}
        />
        {errors.name && (
          <p className="text-xs text-destructive">{errors.name}</p>
        )}
      </div>

      {/* 描述 */}
      <div className="space-y-2">
        <Label className="text-sm font-medium">{tBuilder('description')}</Label>
        <Input
          value={description}
          onChange={e => onDescriptionChange(e.target.value)}
          placeholder={tBuilder('descriptionPlaceholder')}
        />
      </div>

      {/* 启用开关 */}
      <div className="flex items-center gap-3">
        <input
          type="checkbox"
          id="transform-enabled"
          checked={enabled}
          onChange={e => onEnabledChange(e.target.checked)}
          className="h-4 w-4"
        />
        <Label htmlFor="transform-enabled" className="text-sm font-medium cursor-pointer">
          {tBuilder('enableThisTransform')}
        </Label>
      </div>

      {/* Scope Selection */}
      <div className="space-y-4">
        <div className="space-y-2">
          <Label className="text-sm font-medium">{tBuilder('scopeLabel')}</Label>
          <Select value={scopeType} onValueChange={(v: any) => onScopeTypeChange(v)}>
            <SelectTrigger>
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value="global">{tBuilder('scope.global')}</SelectItem>
              <SelectItem value="device_type">{tBuilder('scope.deviceType')}</SelectItem>
              <SelectItem value="device">{tBuilder('scope.device')}</SelectItem>
            </SelectContent>
          </Select>
        </div>

        {scopeType !== 'global' && (
          <div className="space-y-2">
            <Label className="text-sm font-medium">
              {scopeType === 'device_type' ? tBuilder('scope.deviceType') : tBuilder('scope.device')}
            </Label>
            <Select value={scopeValue} onValueChange={onScopeValueChange}>
              <SelectTrigger className={cn(errors.scopeValue && "border-destructive")}>
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
            {errors.scopeValue && (
              <p className="text-xs text-destructive">{errors.scopeValue}</p>
            )}
          </div>
        )}
      </div>
    </div>
  )
}

// ============================================================================
// Step 2: Code
// ============================================================================

interface CodeStepProps {
  jsCode: string
  onCodeChange: (v: string) => void
  templates: Array<{ key: string; name: string; code: string }>
  onApplyTemplate: (code: string) => void
  scopeType: ScopeType
  scopeValue: string
  deviceTypeMetrics?: Array<{ name: string; display_name: string; data_type: string; unit?: string }>
  errors: FormErrors
  outputPrefix: string
  onOutputPrefixChange: (v: string) => void
  t: (key: string) => string
  tBuilder: (key: string) => string
}

function CodeStep({
  jsCode,
  onCodeChange,
  templates,
  onApplyTemplate,
  scopeValue,
  deviceTypeMetrics,
  errors,
  outputPrefix,
  onOutputPrefixChange,
  tBuilder,
}: CodeStepProps) {
  return (
    <div className="h-full flex flex-col">
      <div className="text-center mb-4">
        <h3 className="text-lg font-semibold">{tBuilder('steps.code')}</h3>
        <p className="text-sm text-muted-foreground">{tBuilder('steps.codeDesc')}</p>
      </div>

      {/* Output Prefix */}
      <div className="mb-4">
        <Label className="text-sm font-medium">{tBuilder('outputPrefix')}</Label>
        <Input
          value={outputPrefix}
          onChange={e => onOutputPrefixChange(e.target.value)}
          placeholder={tBuilder('outputPrefixPlaceholder')}
          className={cn(errors.outputPrefix && "border-destructive")}
        />
        <p className="text-xs text-muted-foreground mt-1">{tBuilder('outputPrefixHint')}</p>
        {errors.outputPrefix && (
          <p className="text-xs text-destructive mt-1">{errors.outputPrefix}</p>
        )}
      </div>

      {/* Templates */}
      <div className="mb-4">
        <Label className="text-xs font-medium text-muted-foreground mb-2 block">{tBuilder('codeTemplates')}</Label>
        <div className="flex flex-wrap gap-2">
          {templates.map((tpl) => (
            <Button
              key={tpl.key}
              variant="outline"
              size="sm"
              onClick={() => onApplyTemplate(tpl.code)}
              className="h-8 text-xs"
            >
              {tpl.name}
            </Button>
          ))}
        </div>
      </div>

      {/* Main Code Editor Area */}
      <div className="flex-1 flex gap-4 min-h-0">
        {/* Left - Variables Preview */}
        <div className="min-w-0 max-w-xs border rounded-lg flex flex-col bg-muted/10">
          <div className="px-3 py-2 border-b bg-muted/20 flex items-center gap-2 shrink-0">
            <Database className="h-4 w-4 text-blue-500" />
            <span className="text-sm font-medium">{tBuilder('availableVars')}</span>
          </div>
          <div className="flex-1 overflow-auto p-2">
            {deviceTypeMetrics && deviceTypeMetrics.length > 0 ? (
              <div className="space-y-1.5">
                {deviceTypeMetrics.map((metric, idx) => (
                  <div
                    key={idx}
                    className="p-2 bg-background border rounded text-xs break-all inline-block w-full"
                  >
                    <code className="text-blue-600">input.{metric.name}</code>
                    {metric.display_name && metric.display_name !== metric.name && (
                      <span className="text-muted-foreground ml-1">({metric.display_name})</span>
                    )}
                  </div>
                ))}
              </div>
            ) : (
              <div className="text-center text-muted-foreground text-xs py-4">
                通过 <code className="text-blue-600">input</code> {tBuilder('accessRawData')}
              </div>
            )}
          </div>
        </div>

        {/* Right - Code Editor */}
        <div className="flex-1 flex flex-col min-w-0">
          <Textarea
            value={jsCode}
            onChange={e => onCodeChange(e.target.value)}
            placeholder="// Input data is available via the 'input' variable&#10;// Example:&#10;return {&#10;  transformed_value: input.value * 2&#10;};"
            className="flex-1 resize-none font-mono text-sm rounded-lg bg-muted/30 focus-visible:ring-2 focus-visible:ring-primary focus-visible:ring-offset-2 p-3 min-h-[300px]"
            spellCheck={false}
          />
          {errors.code && (
            <p className="text-xs text-destructive mt-2">{errors.code}</p>
          )}
        </div>
      </div>
    </div>
  )
}

// ============================================================================
// Step 3: Test
// ============================================================================

interface TestStepProps {
  jsCode: string
  testInput: string
  onTestInputChange: (v: string) => void
  testOutput: string
  testError: string
  testRunning: boolean
  onTest: () => void
  onClearTest: () => void
  deviceTypeMetrics?: Array<{ name: string; display_name: string; data_type: string; unit?: string }>
  scopeType: ScopeType
  t: (key: string) => string
  tBuilder: (key: string) => string
}

function TestStep({
  jsCode,
  testInput,
  onTestInputChange,
  testOutput,
  testError,
  testRunning,
  onTest,
  onClearTest,
  deviceTypeMetrics,
  scopeType,
  tBuilder,
}: TestStepProps) {
  const generateMockData = useCallback(() => {
    if (!deviceTypeMetrics || deviceTypeMetrics.length === 0) return

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
    onTestInputChange(JSON.stringify(mockData, null, 2))
  }, [deviceTypeMetrics, onTestInputChange])

  return (
    <div className="space-y-6 max-w-3xl mx-auto py-4">
      <div className="text-center mb-6">
        <h3 className="text-lg font-semibold">{tBuilder('test.title')}</h3>
        <p className="text-sm text-muted-foreground">{tBuilder('test.description')}</p>
      </div>

      {/* Summary */}
      <div className="grid grid-cols-2 gap-4">
        <div className="rounded-lg border bg-card p-4 text-center">
          <div className="text-xl font-bold text-blue-500">
            {jsCode.split('\n').filter(s => s.trim()).length}
          </div>
          <div className="text-xs text-muted-foreground">{tBuilder('test.codeLines')}</div>
        </div>
        <div className="rounded-lg border bg-card p-4 text-center">
          <div className="text-xl font-bold">
            {scopeType === 'global' ? tBuilder('scope.global') : scopeType}
          </div>
          <div className="text-xs text-muted-foreground">{tBuilder('test.scope')}</div>
        </div>
      </div>

      {/* Code Preview */}
      <div className="rounded-lg border bg-card p-4">
        <h4 className="font-medium flex items-center gap-2 mb-3">
          <Code className="h-4 w-4" />
          {tBuilder('test.transformCode')}
        </h4>
        <pre className="text-xs font-mono bg-muted/30 p-3 rounded overflow-x-auto whitespace-pre-wrap max-h-48">
          {jsCode || tBuilder('noCode')}
        </pre>
      </div>

      {/* Test Panel */}
      <div className="rounded-lg border bg-card p-4">
        <h4 className="font-medium flex items-center gap-2 mb-3">
          <Play className="h-4 w-4" />
          {tBuilder('test.testPanel')}
        </h4>

        <div className="space-y-3">
          <div>
            <Label className="text-xs text-muted-foreground mb-2 block">{tBuilder('inputData')}</Label>
            <Textarea
              value={testInput}
              onChange={e => onTestInputChange(e.target.value)}
              placeholder='{"temp": 25}'
              className="font-mono text-xs resize-none bg-muted/30 h-24"
            />
          </div>

          <div className="flex items-center gap-2">
            <Button
              size="sm"
              onClick={onTest}
              disabled={!jsCode || testRunning}
              className="h-8"
            >
              {testRunning ? <Loader2 className="h-3 w-3 animate-spin" /> : <Play className="h-3 w-3 mr-1" />}
              {tBuilder('run')}
            </Button>
            {deviceTypeMetrics && deviceTypeMetrics.length > 0 && (
              <Button
                size="sm"
                variant="outline"
                onClick={generateMockData}
                className="h-8"
              >
                <FlaskConical className="h-3 w-3 mr-1" />
                {tBuilder('generateMock')}
              </Button>
            )}
            {(testOutput || testError) && (
              <Button
                size="sm"
                variant="ghost"
                onClick={onClearTest}
                className="h-8"
              >
                {tBuilder('clear')}
              </Button>
            )}
          </div>

          {/* Output */}
          {(testOutput || testError) && (
            <div>
              <Label className="text-xs text-muted-foreground mb-2 block">{tBuilder('outputData')}</Label>
              <div className="rounded-md bg-muted/30 p-2 max-h-40 overflow-auto">
                {testError && (
                  <div className="p-1.5 bg-destructive/10 border border-destructive/20 rounded text-xs text-destructive font-mono">
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
      </div>
    </div>
  )
}
