import { useState, useEffect } from "react"
import { useTranslation } from "react-i18next"
import {
  Dialog,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogTitle,
  DialogDescription,
} from "@/components/ui/dialog"
import { Button } from "@/components/ui/button"
import { Input } from "@/components/ui/input"
import { Label } from "@/components/ui/label"
import { Textarea } from "@/components/ui/textarea"
import { Badge } from "@/components/ui/badge"
import { Card } from "@/components/ui/card"
import { toast } from "@/components/ui/use-toast"
import {
  Plus,
  Trash2,
  ChevronLeft,
  ChevronRight,
  Check,
  AlertCircle,
  Sparkles,
  Wand2,
  FileText,
  ArrowDown,
  Settings,
  Zap,
  Code,
  Database,
} from "lucide-react"
import { cn } from "@/lib/utils"
import type { DeviceType, MetricDefinition, CommandDefinition } from "@/types"

// Validation result type
interface ValidationResult {
  valid: boolean
  errors?: string[]
  warnings?: string[]
  message: string
}

// Form errors type
interface FormErrors {
  device_type?: string
  name?: string
  metrics?: Record<number, string>
  commands?: Record<number, string>
  [key: string]: string | Record<number, string> | undefined
}

// ============================================================================
// TYPES
// ============================================================================

interface AddDeviceTypeDialogProps {
  open: boolean
  onOpenChange: (open: boolean) => void
  onAdd: (definition: DeviceType) => Promise<boolean>
  onValidate: (definition: DeviceType) => Promise<ValidationResult>
  onGenerateMDL: (deviceName: string, description: string, metricsExample: string, commandsExample: string) => Promise<string>
  adding: boolean
  validating: boolean
  generating: boolean
  // Optional: When provided, dialog operates in edit mode
  editDeviceType?: DeviceType | null
}

type Step = 'basic' | 'data' | 'commands' | 'review' | 'finish'

// ============================================================================
// STEP WIZARD DIALOG
// ============================================================================

export function AddDeviceTypeDialog({
  open,
  onOpenChange,
  onAdd,
  onValidate,
  onGenerateMDL,
  adding,
  validating,
  generating,
  editDeviceType,
}: AddDeviceTypeDialogProps) {
  const { t } = useTranslation(['common', 'devices'])
  const isEditMode = !!editDeviceType

  // Step state
  const [currentStep, setCurrentStep] = useState<Step>('basic')
  const [completedSteps, setCompletedSteps] = useState<Set<Step>>(new Set())

  // Form data
  const [formData, setFormData] = useState<Partial<DeviceType>>({
    device_type: "",
    name: "",
    description: "",
    categories: [],
    mode: "simple",
    metrics: [],
    commands: [],
    uplink_samples: [],
  })

  // UI states
  const [formErrors, setFormErrors] = useState<FormErrors>({})
  const [validationResult, setValidationResult] = useState<ValidationResult | null>(null)
  const [rawSamplesText, setRawSamplesText] = useState("")

  // AI generation states
  const [aiMetricsExample, setAiMetricsExample] = useState("")
  const [aiCommandsExample, setAiCommandsExample] = useState("")
  const [aiCommandDesc, setAiCommandDesc] = useState("")

  // Reset when dialog opens or editDeviceType changes
  useEffect(() => {
    if (open) {
      setCurrentStep('basic')
      setCompletedSteps(new Set())

      if (editDeviceType) {
        // Load existing data for edit mode
        setFormData(editDeviceType)
        // Also load sample data as JSON string
        if (editDeviceType.uplink_samples && editDeviceType.uplink_samples.length > 0) {
          setRawSamplesText(editDeviceType.uplink_samples.map(s => JSON.stringify(s, null, 2)).join('\n'))
        } else {
          setRawSamplesText("")
        }
      } else {
        // Reset to empty for add mode
        setFormData({
          device_type: "",
          name: "",
          description: "",
          categories: [],
          mode: "simple",
          metrics: [],
          commands: [],
          uplink_samples: [],
        })
        setRawSamplesText("")
      }

      setFormErrors({})
      setValidationResult(null)
      setAiMetricsExample("")
      setAiCommandsExample("")
      setAiCommandDesc("")
    }
  }, [open, editDeviceType])

  // Update field (auto-generation now handled in BasicInfoStep on blur)
  const updateField = <K extends keyof DeviceType>(field: K, value: DeviceType[K]) => {
    setFormData(prev => ({ ...prev, [field]: value }))
    // Clear error for this field
    if (formErrors[field as string]) {
      setFormErrors(prev => ({ ...prev, [field]: undefined }))
    }
  }

  // Validate current step
  const validateStep = (step: Step): boolean => {
    const errors: FormErrors = {}

    if (step === 'basic') {
      if (!formData.name?.trim()) {
        errors.name = t('devices:types.validation.nameRequired')
      }
      if (!formData.device_type?.trim()) {
        errors.device_type = t('devices:types.validation.deviceTypeRequired')
      }
    }

    if (step === 'data' && formData.mode === 'full') {
      formData.metrics?.forEach((metric, i) => {
        if (!metric.name?.trim()) {
          if (!errors.metrics) errors.metrics = {}
          errors.metrics[i] = t('devices:types.validation.metricNameRequired')
        }
      })
    }

    if (step === 'commands') {
      formData.commands?.forEach((cmd, i) => {
        if (!cmd.name?.trim()) {
          if (!errors.commands) errors.commands = {}
          errors.commands[i] = t('devices:types.validation.commandNameRequired')
        }
      })
    }

    setFormErrors(errors)
    return Object.keys(errors).length === 0
  }

  // Navigate to next step
  const handleNext = async () => {
    if (!validateStep(currentStep)) return

    const newCompleted = new Set(completedSteps)
    newCompleted.add(currentStep)
    setCompletedSteps(newCompleted)

    const steps: Step[] = ['basic', 'data', 'commands', 'review', 'finish']
    const currentIndex = steps.indexOf(currentStep)
    if (currentIndex < steps.length - 1) {
      setCurrentStep(steps[currentIndex + 1])
    }
  }

  // Navigate to previous step
  const handlePrevious = () => {
    const steps: Step[] = ['basic', 'data', 'commands', 'review', 'finish']
    const currentIndex = steps.indexOf(currentStep)
    if (currentIndex > 0) {
      setCurrentStep(steps[currentIndex - 1])
    }
  }

  // Skip current step (for optional steps)
  const handleSkip = () => {
    const steps: Step[] = ['basic', 'data', 'commands', 'review', 'finish']
    const currentIndex = steps.indexOf(currentStep)
    if (currentIndex < steps.length - 1) {
      setCurrentStep(steps[currentIndex + 1])
    }
  }

  // Generate metrics from AI
  const handleGenerateMetrics = async () => {
    if (!aiMetricsExample.trim()) {
      toast({
        title: t('devices:types.validationError'),
        description: "Please paste sample data first",
        variant: "destructive",
      })
      return
    }

    try {
      const result = await onGenerateMDL(
        formData.name || "device",
        formData.description || "",
        aiMetricsExample,
        "" // No commands for metrics generation
      )
      const parsed = JSON.parse(result)

      setFormData(prev => ({
        ...prev,
        metrics: parsed.uplink?.metrics || parsed.metrics || [],
      }))

      toast({
        title: t('common:success'),
        description: `Generated ${parsed.metrics?.length || 0} metrics`,
      })
    } catch (e) {
      toast({
        title: t('devices:types.generate.failed'),
        description: (e as Error).message,
        variant: "destructive",
      })
    }
  }

  // Generate commands from AI
  const handleGenerateCommands = async () => {
    if (!aiCommandsExample.trim() && !aiCommandDesc.trim()) {
      toast({
        title: t('devices:types.validationError'),
        description: "Please paste sample data or describe the command",
        variant: "destructive",
      })
      return
    }

    try {
      const result = await onGenerateMDL(
        formData.name || "device",
        formData.description || "",
        "", // No metrics for commands generation
        aiCommandsExample || JSON.stringify({ description: aiCommandDesc })
      )
      const parsed = JSON.parse(result)

      setFormData(prev => ({
        ...prev,
        commands: parsed.downlink?.commands || parsed.commands || [],
      }))

      toast({
        title: t('common:success'),
        description: `Generated ${parsed.commands?.length || 0} commands`,
      })
    } catch (e) {
      toast({
        title: t('devices:types.generate.failed'),
        description: (e as Error).message,
        variant: "destructive",
      })
    }
  }

  // Final save
  const handleSave = async () => {
    const definition: DeviceType = {
      device_type: formData.device_type!,
      name: formData.name!,
      description: formData.description || "",
      categories: formData.categories || [],
      mode: formData.mode || "simple",
      metrics: formData.metrics || [],
      commands: formData.commands || [],
      uplink_samples: formData.uplink_samples || [],
    }

    const success = await onAdd(definition)
    if (success) {
      setCurrentStep('finish')
    }
  }

  // Step navigation config
  const steps: { key: Step; label: string; icon: React.ReactNode }[] = [
    { key: 'basic', label: 'Basic Info', icon: <Settings className="h-4 w-4" /> },
    { key: 'data', label: 'Data Definition', icon: <ArrowDown className="h-4 w-4" /> },
    { key: 'commands', label: 'Commands', icon: <FileText className="h-4 w-4" /> },
    { key: 'review', label: 'Review', icon: <Check className="h-4 w-4" /> },
    { key: 'finish', label: 'Finish', icon: <Sparkles className="h-4 w-4" /> },
  ]

  const stepIndex = steps.findIndex(s => s.key === currentStep)
  const isFirstStep = currentStep === 'basic'

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-4xl h-[90vh] max-h-[90vh] flex flex-col p-0 overflow-hidden">
        {/* Header with Steps */}
        <DialogHeader className="px-6 pt-6 pb-2 border-b space-y-4">
          <DialogTitle className="text-xl">
            {isEditMode ? 'Edit Device Type' : t('devices:types.add.title')}
          </DialogTitle>

          {/* Step Indicator */}
          <div className="flex items-center justify-between">
            {steps.map((step, index) => {
              const isCompleted = completedSteps.has(step.key)
              const isCurrent = step.key === currentStep
              const isPast = index < stepIndex

              return (
                <div key={step.key} className="flex items-center gap-2 flex-1">
                  <div
                    className={cn(
                      "w-8 h-8 rounded-full flex items-center justify-center text-sm font-medium transition-colors shrink-0",
                      isCompleted && "bg-primary text-primary-foreground",
                      isCurrent && "bg-primary text-primary-foreground ring-4 ring-primary/20",
                      !isCompleted && !isCurrent && "bg-muted text-muted-foreground"
                    )}
                  >
                    {isCompleted ? <Check className="h-4 w-4" /> : index + 1}
                  </div>
                  <span
                    className={cn(
                      "text-xs font-medium whitespace-nowrap overflow-hidden text-ellipsis",
                      isCurrent ? "text-primary" : "text-muted-foreground"
                    )}
                  >
                    {step.label}
                  </span>
                  {index < steps.length - 1 && (
                    <div
                      className={cn(
                        "flex-1 h-0.5 transition-colors",
                        isPast ? "bg-primary" : "bg-muted"
                      )}
                    />
                  )}
                </div>
              )
            })}
          </div>
        </DialogHeader>

        {/* Step Content */}
        <div className="flex-1 overflow-y-auto px-6 py-4">
          {currentStep === 'basic' && (
            <BasicInfoStep
              data={formData}
              onChange={updateField}
              errors={formErrors}
            />
          )}

          {currentStep === 'data' && (
            <DataDefinitionStep
              data={formData}
              onChange={updateField}
              errors={formErrors}
              rawSamplesText={rawSamplesText}
              onRawSamplesChange={setRawSamplesText}
              onGenerateMetrics={handleGenerateMetrics}
              aiExample={aiMetricsExample}
              onAiExampleChange={setAiMetricsExample}
              generating={generating}
            />
          )}

          {currentStep === 'commands' && (
            <CommandsStep
              data={formData}
              onChange={setFormData}
              errors={formErrors}
              onGenerateCommands={handleGenerateCommands}
              aiExample={aiCommandsExample}
              onAiExampleChange={setAiCommandsExample}
              aiDesc={aiCommandDesc}
              onAiDescChange={setAiCommandDesc}
              generating={generating}
            />
          )}

          {currentStep === 'review' && (
            <ReviewStep
              data={formData as DeviceType}
              onEdit={(step) => setCurrentStep(step)}
              onValidate={async () => {
                const result = await onValidate(formData as DeviceType)
                setValidationResult(result)
                return result
              }}
              validating={validating}
              validationResult={validationResult}
            />
          )}

          {currentStep === 'finish' && (
            <FinishStep
              deviceType={formData.device_type || ""}
              onOpenChange={onOpenChange}
              isEditMode={isEditMode}
            />
          )}
        </div>

        {/* Footer Navigation */}
        {currentStep !== 'finish' && (
          <DialogFooter className="px-6 pb-6 pt-4 border-t gap-2">
            {!isFirstStep && (
              <Button variant="outline" onClick={handlePrevious}>
                <ChevronLeft className="h-4 w-4 mr-1" />
                {t('common:previous')}
              </Button>
            )}

            <div className="flex-1" />

            {/* Skip button for optional steps */}
            {(currentStep === 'data' || currentStep === 'commands') && (
              <Button variant="ghost" onClick={handleSkip}>
                Skip this step
              </Button>
            )}

            {currentStep === 'review' ? (
              <>
                <Button variant="outline" onClick={handleSave} disabled={adding}>
                  {adding ? (isEditMode ? 'Saving...' : t('devices:types.adding')) : (isEditMode ? 'Save Changes' : t('common:save'))}
                </Button>
              </>
            ) : (
              <Button onClick={handleNext}>
                {t('common:next')}
                <ChevronRight className="h-4 w-4 ml-1" />
              </Button>
            )}
          </DialogFooter>
        )}
      </DialogContent>
    </Dialog>
  )
}

// ============================================================================
// STEP 1: Basic Info
// ============================================================================

interface BasicInfoStepProps {
  data: Partial<DeviceType>
  onChange: <K extends keyof DeviceType>(field: K, value: DeviceType[K]) => void
  errors: FormErrors
}

function BasicInfoStep({ data, onChange, errors }: BasicInfoStepProps) {
  const [categoryInput, setCategoryInput] = useState("")
  const [nameInput, setNameInput] = useState(data.name || "")

  // Sync nameInput with data.name when it changes (e.g., when switching to edit mode)
  useEffect(() => {
    setNameInput(data.name || "")
  }, [data.name])

  const addCategory = () => {
    const cat = categoryInput.trim()
    if (cat && !data.categories?.includes(cat)) {
      onChange('categories', [...(data.categories || []), cat])
      setCategoryInput("")
    }
  }

  const removeCategory = (cat: string) => {
    onChange('categories', (data.categories || []).filter(c => c !== cat))
  }

  // Generate type ID from name
  const generateTypeId = (name: string): string => {
    return name.toLowerCase()
      .replace(/\s+/g, "_")
      .replace(/[^a-z0-9_]/g, "")
      .replace(/_+/g, "_")
      .replace(/^_|_$/g, "")
  }

  // Only auto-generate on blur (when user finishes typing)
  const handleNameBlur = () => {
    if (!data.device_type && nameInput.trim()) {
      onChange('device_type', generateTypeId(nameInput))
    }
  }

  const handleNameChange = (value: string) => {
    setNameInput(value)
    onChange('name', value)
  }

  return (
    <div className="space-y-6 max-w-2xl mx-auto py-4">
      <div className="text-center mb-6">
        <h3 className="text-lg font-semibold">Basic Information</h3>
        <p className="text-sm text-muted-foreground">Enter the basic information for your device type</p>
      </div>

      {/* Device Type (name) */}
      <div className="space-y-2">
        <Label htmlFor="device-type-name" className="text-sm font-medium">
          Device Type <span className="text-destructive">*</span>
        </Label>
        <Input
          id="device-type-name"
          value={nameInput}
          onChange={(e) => handleNameChange(e.target.value)}
          onBlur={handleNameBlur}
          placeholder="e.g., Smart Temperature Sensor"
          className={cn(errors.name && "border-destructive")}
        />
        {errors.name && (
          <p className="text-xs text-destructive flex items-center gap-1">
            <AlertCircle className="h-3 w-3" />
            {errors.name}
          </p>
        )}
      </div>

      {/* Type ID (auto-generated from Device Type) */}
      <div className="space-y-2">
        <Label htmlFor="type-id" className="text-sm font-medium">
          Type ID <span className="text-destructive">*</span>
        </Label>
        <Input
          id="type-id"
          value={data.device_type || ""}
          onChange={(e) => onChange('device_type', e.target.value)}
          placeholder="smart_temp_sensor"
          className={cn("font-mono", errors.device_type && "border-destructive")}
        />
        <p className="text-xs text-muted-foreground">
          Auto-generated from Device Type after you finish typing
        </p>
        {errors.device_type && (
          <p className="text-xs text-destructive flex items-center gap-1">
            <AlertCircle className="h-3 w-3" />
            {errors.device_type}
          </p>
        )}
      </div>

      {/* Description */}
      <div className="space-y-2">
        <Label htmlFor="description" className="text-sm font-medium">Description</Label>
        <Textarea
          id="description"
          value={data.description || ""}
          onChange={(e) => onChange('description', e.target.value)}
          placeholder="Describe what this device type does..."
          rows={3}
          className="resize-none"
        />
      </div>

      {/* Categories */}
      <div className="space-y-2">
        <Label className="text-sm font-medium">Categories</Label>
        <div className="flex gap-2 flex-wrap">
          {data.categories?.map((cat, i) => (
            <Badge key={i} variant="secondary" className="pl-2 pr-1 h-7">
              {cat}
              <button
                onClick={() => removeCategory(cat)}
                className="ml-1 hover:text-destructive"
              >
                ×
              </button>
            </Badge>
          ))}
          <div className="flex gap-1">
            <Input
              placeholder="+ Add category"
              value={categoryInput}
              onChange={(e) => setCategoryInput(e.target.value)}
              onKeyDown={(e) => e.key === 'Enter' && (e.preventDefault(), addCategory())}
              className="h-7 w-32 text-xs"
            />
          </div>
        </div>
      </div>
    </div>
  )
}

// ============================================================================
// STEP 2: Data Definition
// ============================================================================

interface DataDefinitionStepProps {
  data: Partial<DeviceType>
  onChange: <K extends keyof DeviceType>(field: K, value: DeviceType[K]) => void
  errors: FormErrors
  rawSamplesText: string
  onRawSamplesChange: (value: string) => void
  onGenerateMetrics: () => void
  aiExample: string
  onAiExampleChange: (value: string) => void
  generating: boolean
}

function DataDefinitionStep({
  data,
  onChange,
  errors,
  rawSamplesText,
  onRawSamplesChange,
  onGenerateMetrics,
  aiExample,
  onAiExampleChange,
  generating,
}: DataDefinitionStepProps) {
  const isRawMode = data.mode === 'simple'

  // Add metric
  const addMetric = () => {
    const metrics = data.metrics || []
    onChange('metrics', [
      ...metrics,
      {
        name: `metric_${metrics.length + 1}`,
        display_name: `Metric ${metrics.length + 1}`,
        data_type: "float",
      },
    ])
  }

  // Update metric
  const updateMetric = (index: number, metric: MetricDefinition) => {
    const metrics = data.metrics || []
    const newMetrics = [...metrics]
    newMetrics[index] = metric
    onChange('metrics', newMetrics)
  }

  // Remove metric
  const removeMetric = (index: number) => {
    const metrics = data.metrics || []
    onChange('metrics', metrics.filter((_, i) => i !== index))
  }

  // Parse raw samples
  const parseRawSamples = () => {
    const lines = rawSamplesText.trim().split('\n').filter(l => l.trim())
    const samples: Record<string, unknown>[] = []
    for (const line of lines) {
      try {
        samples.push(JSON.parse(line))
      } catch {
        // Skip invalid JSON
      }
    }
    onChange('uplink_samples', samples)
  }

  return (
    <div className="space-y-6 py-4">
      <div className="text-center mb-6">
        <h3 className="text-lg font-semibold">Data Definition (Uplink)</h3>
        <p className="text-sm text-muted-foreground">Define how device data is parsed and stored</p>
      </div>

      {/* Mode Selection */}
      <div className="flex justify-center gap-4">
        <button
          onClick={() => onChange('mode', 'full')}
          className={cn(
            "flex-1 max-w-xs p-4 rounded-lg border-2 transition-all text-left",
            !isRawMode
              ? "border-primary bg-primary/5"
              : "border-muted hover:border-muted-foreground/30"
          )}
        >
          <div className="flex items-center gap-3">
            <div className={cn(
              "p-2 rounded-lg",
              !isRawMode ? "bg-primary text-primary-foreground" : "bg-muted"
            )}>
              <Settings className="h-5 w-5" />
            </div>
            <div>
              <p className={cn("font-medium", !isRawMode ? "text-foreground" : "text-muted-foreground")}>
                Define Metrics
              </p>
              <p className="text-xs text-muted-foreground">Parse & store each field</p>
            </div>
          </div>
        </button>

        <button
          onClick={() => onChange('mode', 'simple')}
          className={cn(
            "flex-1 max-w-xs p-4 rounded-lg border-2 transition-all text-left",
            isRawMode
              ? "border-primary bg-primary/5"
              : "border-muted hover:border-muted-foreground/30"
          )}
        >
          <div className="flex items-center gap-3">
            <div className={cn(
              "p-2 rounded-lg",
              isRawMode ? "bg-primary text-primary-foreground" : "bg-muted"
            )}>
              <Zap className="h-5 w-5" />
            </div>
            <div>
              <p className={cn("font-medium", isRawMode ? "text-foreground" : "text-muted-foreground")}>
                Raw Data Mode
              </p>
              <p className="text-xs text-muted-foreground">Store payload as-is</p>
            </div>
          </div>
        </button>
      </div>

      {/* Define Metrics Mode */}
      {!isRawMode && (
        <div className="flex flex-col h-full space-y-4">
          {/* Toolbar: AI Generate */}
          <div className="flex flex-wrap items-center gap-3 p-4 rounded-lg border bg-muted/30">
            <div className="flex items-center gap-2 text-sm shrink-0">
              <Sparkles className="h-4 w-4 text-purple-500" />
              <span className="font-medium">AI Generate:</span>
            </div>
            <Textarea
              value={aiExample}
              onChange={(e) => onAiExampleChange(e.target.value)}
              placeholder='{"temperature": 25.5, "humidity": 60}'
              rows={2}
              className="flex-1 min-w-[200px] font-mono text-sm h-auto max-h-20"
            />
            <Button
              onClick={onGenerateMetrics}
              disabled={!aiExample.trim() || generating}
              variant="secondary"
              size="sm"
              className="shrink-0"
            >
              {generating ? (
                <>
                  <Sparkles className="mr-2 h-4 w-4 animate-spin" />
                  Generating...
                </>
              ) : (
                <>
                  <Wand2 className="mr-2 h-4 w-4" />
                  Generate
                </>
              )}
            </Button>
          </div>

          {/* Manual Entry List */}
          <div className="flex-1 flex flex-col min-h-0">
            <div className="flex items-center justify-between mb-3">
              <h4 className="text-sm font-medium">Metrics ({data.metrics?.length || 0})</h4>
              <div className="flex items-center gap-2">
                <Button variant="outline" size="sm" className="gap-1 h-8">
                  <Code className="h-3 w-3" />
                  Import from JSON
                </Button>
                <Button onClick={addMetric} size="sm" variant="outline" className="h-8">
                  <Plus className="mr-1 h-3 w-3" />
                  Add Metric
                </Button>
              </div>
            </div>

            {(!data.metrics || data.metrics.length === 0) ? (
              <div className="flex-1 flex items-center justify-center border-2 border-dashed rounded-lg bg-muted/20">
                <div className="text-center py-12">
                  <FileText className="h-8 w-8 mx-auto mb-2 text-muted-foreground" />
                  <p className="text-sm text-muted-foreground">No metrics defined</p>
                  <p className="text-xs text-muted-foreground mt-1">Use AI Generate above or add metrics manually</p>
                </div>
              </div>
            ) : (
              <div className="flex-1 overflow-y-auto space-y-2 pr-1">
                {data.metrics.map((metric, i) => (
                  <MetricEditorCompact
                    key={i}
                    metric={metric}
                    onChange={(m) => updateMetric(i, m)}
                    onRemove={() => removeMetric(i)}
                    error={errors.metrics?.[i]}
                  />
                ))}
              </div>
            )}
          </div>
        </div>
      )}

      {/* Raw Data Mode */}
      {isRawMode && (
        <div className="max-w-2xl mx-auto space-y-4">
          <div className="rounded-lg border bg-muted/30 p-6 text-center">
            <div className="w-12 h-12 rounded-full bg-muted flex items-center justify-center mx-auto mb-4">
              <Zap className="h-6 w-6 text-muted-foreground" />
            </div>
            <h4 className="font-medium mb-2">Raw Data Mode</h4>
            <p className="text-sm text-muted-foreground mb-4">
              Payloads will be stored as-is without parsing. You can view raw data in device details.
            </p>
          </div>

          <div className="space-y-2">
            <Label className="text-sm font-medium">
              Sample Data for AI Reference (Optional)
            </Label>
            <p className="text-xs text-muted-foreground">
              This sample data helps the AI understand your payload structure when generating metrics or commands
            </p>
            <Textarea
              value={rawSamplesText}
              onChange={(e) => onRawSamplesChange(e.target.value)}
              onBlur={parseRawSamples}
              placeholder='{"temperature": 25.5, "humidity": 60, "battery": 85}\n{"temperature": 26.2, "humidity": 58, "battery": 82}'
              rows={6}
              className="font-mono text-sm"
            />
          </div>
        </div>
      )}
    </div>
  )
}

// ============================================================================
// STEP 3: Commands
// ============================================================================

interface CommandsStepProps {
  data: Partial<DeviceType>
  onChange: (data: Partial<DeviceType>) => void
  errors: FormErrors
  onGenerateCommands: () => void
  aiExample: string
  onAiExampleChange: (value: string) => void
  aiDesc: string
  onAiDescChange: (value: string) => void
  generating: boolean
}

function CommandsStep({
  data,
  onChange,
  errors,
  onGenerateCommands,
  aiExample,
  onAiExampleChange,
  aiDesc,
  onAiDescChange,
  generating,
}: CommandsStepProps) {
  // Add command
  const addCommand = () => {
    const commands = data.commands || []
    onChange({
      ...data,
      commands: [
        ...commands,
        {
          name: `cmd_${commands.length + 1}`,
          display_name: `Command ${commands.length + 1}`,
          payload_template: '{"action": "${value}"}',
          parameters: [],
        },
      ],
    })
  }

  // Update command
  const updateCommand = (index: number, command: CommandDefinition) => {
    const commands = data.commands || []
    const newCommands = [...commands]
    newCommands[index] = command
    onChange({ ...data, commands: newCommands })
  }

  // Remove command
  const removeCommand = (index: number) => {
    const commands = data.commands || []
    onChange({ ...data, commands: commands.filter((_, i) => i !== index) })
  }

  return (
    <div className="space-y-6 py-4">
      <div className="text-center mb-2">
        <h3 className="text-lg font-semibold">Commands (Downlink)</h3>
        <p className="text-sm text-muted-foreground">Define commands that can be sent to the device</p>
      </div>

      {/* AI Generate Toolbar */}
      <div className="flex flex-wrap items-center gap-3 p-4 rounded-lg border bg-muted/30">
        <div className="flex items-center gap-2 text-sm shrink-0">
          <Sparkles className="h-4 w-4 text-purple-500" />
          <span className="font-medium">AI Generate:</span>
        </div>

        <Textarea
          value={aiExample}
          onChange={(e) => onAiExampleChange(e.target.value)}
          placeholder='{"action": "set_interval", "interval": 60}'
          rows={2}
          className="flex-1 min-w-[200px] font-mono text-sm h-auto max-h-20"
        />

        <div className="flex items-center gap-1 text-xs text-muted-foreground shrink-0">
          <div className="h-px w-8 bg-muted" />
          <span>OR</span>
          <div className="h-px w-8 bg-muted" />
        </div>

        <Textarea
          value={aiDesc}
          onChange={(e) => onAiDescChange(e.target.value)}
          placeholder="Describe the command..."
          rows={2}
          className="flex-1 min-w-[200px] text-sm h-auto max-h-20"
        />

        <Button
          onClick={onGenerateCommands}
          disabled={(!aiExample.trim() && !aiDesc.trim()) || generating}
          variant="secondary"
          size="sm"
          className="shrink-0"
        >
          {generating ? (
            <>
              <Sparkles className="mr-2 h-4 w-4 animate-spin" />
              Generating...
            </>
          ) : (
            <>
              <Wand2 className="mr-2 h-4 w-4" />
              Generate
            </>
          )}
        </Button>

        <Button variant="outline" size="sm" className="shrink-0">
          <Code className="mr-1 h-3 w-3" />
          Import JSON
        </Button>
      </div>

      {/* Manual Entry List */}
      <div className="space-y-4">
        <div className="flex items-center justify-between">
          <h4 className="text-sm font-medium flex items-center gap-2">
            <FileText className="h-4 w-4" />
            Commands ({data.commands?.length || 0})
          </h4>
          <Button onClick={addCommand} size="sm" variant="outline" className="h-8">
            <Plus className="mr-1 h-3 w-3" />
            Add Command
          </Button>
        </div>

        {(!data.commands || data.commands.length === 0) ? (
          <div className="text-center py-12 border-2 border-dashed rounded-lg bg-muted/20">
            <FileText className="h-10 w-10 mx-auto text-muted-foreground/50 mb-3" />
            <p className="text-sm text-muted-foreground">No commands defined</p>
            <p className="text-xs text-muted-foreground mt-1">
              Use AI Generate above or add commands manually
            </p>
          </div>
        ) : (
          <div className="space-y-2 max-h-80 overflow-y-auto">
            {data.commands.map((cmd, i) => (
              <CommandEditorCompact
                key={i}
                command={cmd}
                onChange={(c) => updateCommand(i, c)}
                onRemove={() => removeCommand(i)}
                error={errors.commands?.[i]}
              />
            ))}
          </div>
        )}
      </div>
    </div>
  )
}

// ============================================================================
// STEP 4: Review
// ============================================================================

interface ReviewStepProps {
  data: DeviceType
  onEdit: (step: Step) => void
  onValidate: () => Promise<ValidationResult>
  validating: boolean
  validationResult: ValidationResult | null
}

function ReviewStep({ data, onEdit, onValidate, validating, validationResult }: ReviewStepProps) {
  const handleValidate = async () => {
    await onValidate()
  }

  return (
    <div className="space-y-6 max-w-3xl mx-auto py-4">
      <div className="text-center mb-6">
        <h3 className="text-lg font-semibold">Review & Confirm</h3>
        <p className="text-sm text-muted-foreground">Review your device type before saving</p>
      </div>

      {/* Summary Cards */}
      <div className="grid grid-cols-3 gap-4 mb-6">
        <div className="rounded-lg border bg-card p-4 text-center">
          <div className="text-2xl font-bold text-primary">{data.metrics?.length || 0}</div>
          <div className="text-xs text-muted-foreground">Metrics</div>
        </div>
        <div className="rounded-lg border bg-card p-4 text-center">
          <div className="text-2xl font-bold text-blue-500">{data.commands?.length || 0}</div>
          <div className="text-xs text-muted-foreground">Commands</div>
        </div>
        <div className="rounded-lg border bg-card p-4 text-center">
          <div className="text-2xl font-bold text-green-500">
            {data.mode === 'simple' ? 'Raw' : 'Full'}
          </div>
          <div className="text-xs text-muted-foreground">Mode</div>
        </div>
      </div>

      {/* Basic Info */}
      <div className="rounded-lg border bg-card p-4">
        <div className="flex items-center justify-between mb-3">
          <h4 className="font-medium flex items-center gap-2">
            <Settings className="h-4 w-4" />
            Basic Info
          </h4>
          <Button variant="ghost" size="sm" onClick={() => onEdit('basic')}>
            Edit
          </Button>
        </div>
        <div className="grid grid-cols-2 gap-4 text-sm">
          <div>
            <span className="text-muted-foreground">Name:</span>
            <span className="ml-2 font-medium">{data.name}</span>
          </div>
          <div>
            <span className="text-muted-foreground">Type ID:</span>
            <span className="ml-2 font-mono">{data.device_type}</span>
          </div>
          <div className="col-span-2">
            <span className="text-muted-foreground">Description:</span>
            <span className="ml-2">{data.description || '-'}</span>
          </div>
          <div className="col-span-2">
            <span className="text-muted-foreground">Categories:</span>
            <div className="ml-2 inline-flex gap-1">
              {data.categories.length > 0 ? (
                data.categories.map((cat, i) => (
                  <Badge key={i} variant="secondary">{cat}</Badge>
                ))
              ) : (
                <span className="text-muted-foreground">-</span>
              )}
            </div>
          </div>
        </div>
      </div>

      {/* Metrics */}
      <div className="rounded-lg border bg-card p-4">
        <div className="flex items-center justify-between mb-4">
          <h4 className="font-medium flex items-center gap-2">
            <ArrowDown className="h-4 w-4 text-green-500" />
            Metrics ({data.metrics?.length || 0})
          </h4>
          <Button variant="ghost" size="sm" onClick={() => onEdit('data')}>
            Edit
          </Button>
        </div>
        {(!data.metrics || data.metrics.length === 0) ? (
          <p className="text-sm text-muted-foreground">
            {data.mode === 'simple' ? 'Raw Data Mode - no metrics defined' : 'No metrics defined'}
          </p>
        ) : (
          <div className="space-y-3">
            {data.metrics.map((metric, i) => (
              <div key={i} className="p-3 bg-muted/50 rounded-lg">
                <div className="flex items-center justify-between">
                  <div>
                    <span className="font-mono text-sm">{metric.name}</span>
                    <span className="text-muted-foreground mx-2">•</span>
                    <span className="text-sm">{metric.display_name}</span>
                  </div>
                  <Badge variant="outline" className="text-xs">{metric.data_type}</Badge>
                </div>
              </div>
            ))}
          </div>
        )}
      </div>

      {/* Commands */}
      <div className="rounded-lg border bg-card p-4">
        <div className="flex items-center justify-between mb-3">
          <h4 className="font-medium flex items-center gap-2">
            <FileText className="h-4 w-4 text-blue-500" />
            Commands ({data.commands?.length || 0})
          </h4>
          <Button variant="ghost" size="sm" onClick={() => onEdit('commands')}>
            Edit
          </Button>
        </div>
        {(!data.commands || data.commands.length === 0) ? (
          <p className="text-sm text-muted-foreground">No commands defined</p>
        ) : (
          <div className="space-y-2">
            {data.commands.map((cmd, i) => (
              <div key={i} className="text-sm p-2 bg-muted/50 rounded flex items-center justify-between">
                <div>
                  <span className="font-mono">{cmd.name}</span>
                  <span className="text-muted-foreground mx-2">•</span>
                  <span>{cmd.display_name}</span>
                </div>
                <Badge variant="secondary" className="text-xs">
                  {cmd.parameters.length} params
                </Badge>
              </div>
            ))}
          </div>
        )}
      </div>

      {/* Validation */}
      <div className="rounded-lg border bg-card p-4">
        <div className="flex items-center justify-between mb-3">
          <h4 className="font-medium">Validation</h4>
          <Button
            variant="outline"
            size="sm"
            onClick={handleValidate}
            disabled={validating}
          >
            {validating ? 'Validating...' : 'Validate Definition'}
          </Button>
        </div>
        {validationResult && (
          <div className={cn(
            "p-3 rounded-lg text-sm",
            validationResult.valid ? "bg-green-50 text-green-700 dark:bg-green-950/30 dark:text-green-400" : "bg-destructive/10 text-destructive"
          )}>
            <div className="flex items-center gap-2 font-medium">
              {validationResult.valid ? <Check className="h-4 w-4" /> : <AlertCircle className="h-4 w-4" />}
              {validationResult.message}
            </div>
            {validationResult.errors && validationResult.errors.length > 0 && (
              <ul className="mt-2 ml-6 list-disc space-y-1">
                {validationResult.errors.map((err, i) => <li key={i}>{err}</li>)}
              </ul>
            )}
          </div>
        )}
      </div>
    </div>
  )
}

// ============================================================================
// STEP 5: Finish
// ============================================================================

interface FinishStepProps {
  deviceType: string
  onOpenChange: (open: boolean) => void
  isEditMode?: boolean
}

function FinishStep({ deviceType, onOpenChange, isEditMode = false }: FinishStepProps) {
  const { t } = useTranslation(['common', 'devices'])

  return (
    <div className="flex flex-col items-center justify-center h-full py-8">
      <div className="w-16 h-16 rounded-full bg-green-100 dark:bg-green-900/30 flex items-center justify-center mb-6">
        <Check className="h-8 w-8 text-green-600 dark:text-green-400" />
      </div>
      <h3 className="text-xl font-semibold mb-2">
        {isEditMode ? 'Device Type Updated Successfully!' : 'Device Type Added Successfully!'}
      </h3>
      <p className="text-muted-foreground mb-6">
        {isEditMode ? (
          <>The device type <code className="px-2 py-0.5 bg-muted rounded">{deviceType}</code> has been updated.</>
        ) : (
          <>The device type <code className="px-2 py-0.5 bg-muted rounded">{deviceType}</code> has been registered.</>
        )}
      </p>
      <Button onClick={() => onOpenChange(false)}>
        {t('common:close')}
      </Button>
    </div>
  )
}

// ============================================================================
// COMPACT EDITORS
// ============================================================================

function MetricEditorCompact({
  metric,
  onChange,
  onRemove,
  error,
}: {
  metric: MetricDefinition
  onChange: (metric: MetricDefinition) => void
  onRemove: () => void
  error?: string
}) {
  return (
    <div className={cn(
      "rounded-lg border p-3 space-y-2",
      error && "border-destructive"
    )}>
      <div className="flex items-center justify-between">
        <div className="flex items-center gap-2">
          <span className="font-mono text-sm">{metric.name}</span>
          <Badge variant="outline" className="text-xs">{metric.data_type}</Badge>
          {metric.unit && <span className="text-xs text-muted-foreground">{metric.unit}</span>}
        </div>
        <Button variant="ghost" size="icon" onClick={onRemove} className="h-6 w-6">
          <Trash2 className="h-3 w-3 text-destructive" />
        </Button>
      </div>
      <div className="grid grid-cols-2 gap-2">
        <Input
          value={metric.display_name}
          onChange={(e) => onChange({ ...metric, display_name: e.target.value })}
          placeholder="Display name"
          className="h-8 text-sm"
        />
        <Input
          value={metric.unit || ""}
          onChange={(e) => onChange({ ...metric, unit: e.target.value })}
          placeholder="Unit"
          className="h-8 text-sm"
        />
      </div>
      {error && <p className="text-xs text-destructive">{error}</p>}
    </div>
  )
}

function CommandEditorCompact({
  command,
  onChange,
  onRemove,
  error,
}: {
  command: CommandDefinition
  onChange: (command: CommandDefinition) => void
  onRemove: () => void
  error?: string
}) {
  const [expanded, setExpanded] = useState(false)

  return (
    <div className={cn(
      "rounded-lg border p-3 space-y-2",
      error && "border-destructive"
    )}>
      <div className="flex items-center justify-between">
        <div className="flex items-center gap-2">
          <span className="font-mono text-sm">{command.name}</span>
          <Badge variant="secondary" className="text-xs">
            {command.parameters.length} params
          </Badge>
        </div>
        <div className="flex items-center gap-1">
          <Button
            variant="ghost"
            size="icon"
            onClick={() => setExpanded(!expanded)}
            className="h-6 w-6"
          >
            {expanded ? <ChevronLeft className="h-3 w-3" /> : <ChevronRight className="h-3 w-3" />}
          </Button>
          <Button variant="ghost" size="icon" onClick={onRemove} className="h-6 w-6">
            <Trash2 className="h-3 w-3 text-destructive" />
          </Button>
        </div>
      </div>

      {expanded && (
        <div className="space-y-2 pt-2 border-t">
          <Input
            value={command.display_name}
            onChange={(e) => onChange({ ...command, display_name: e.target.value })}
            placeholder="Display name"
            className="h-8 text-sm"
          />
          <Input
            value={command.payload_template}
            onChange={(e) => onChange({ ...command, payload_template: e.target.value })}
            placeholder='{"action": "${value}"}'
            className="h-8 text-sm font-mono"
          />
        </div>
      )}
      {error && <p className="text-xs text-destructive">{error}</p>}
    </div>
  )
}

// ============================================================================
// VIEW DEVICE TYPE DIALOG
// ============================================================================

interface ViewDeviceTypeDialogProps {
  open: boolean
  onOpenChange: (open: boolean) => void
  deviceType: DeviceType | null
}

export function ViewDeviceTypeDialog({ open, onOpenChange, deviceType }: ViewDeviceTypeDialogProps) {
  if (!deviceType) return null

  const isRawMode = deviceType.mode === 'simple'

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-4xl h-[85vh] max-h-[85vh] flex flex-col">
        <DialogHeader className="px-6 pt-6 pb-4 border-b">
          <div className="flex items-center justify-between">
            <div>
              <DialogTitle className="text-xl">{deviceType.name}</DialogTitle>
              <DialogDescription className="flex items-center gap-2 mt-1">
                <code className="text-xs bg-muted px-2 py-0.5 rounded">{deviceType.device_type}</code>
                {isRawMode && (
                  <Badge variant="secondary" className="text-xs">
                    <Zap className="h-3 w-3 mr-1" />
                    Raw Data Mode
                  </Badge>
                )}
              </DialogDescription>
            </div>
            <Button variant="outline" size="sm" onClick={() => onOpenChange(false)}>
              Close
            </Button>
          </div>
        </DialogHeader>

        <div className="flex-1 overflow-y-auto">
          <div className="p-6 space-y-6">
            {/* Summary Cards */}
            <div className="grid grid-cols-3 gap-4">
              <Card className="p-4">
                <div className="flex items-center gap-3">
                  <div className="p-2 rounded-lg bg-green-100 dark:bg-green-900/30">
                    <ArrowDown className="h-5 w-5 text-green-600 dark:text-green-400" />
                  </div>
                  <div>
                    <div className="text-2xl font-bold">{deviceType.metrics?.length || 0}</div>
                    <div className="text-xs text-muted-foreground">Metrics</div>
                  </div>
                </div>
              </Card>
              <Card className="p-4">
                <div className="flex items-center gap-3">
                  <div className="p-2 rounded-lg bg-blue-100 dark:bg-blue-900/30">
                    <FileText className="h-5 w-5 text-blue-600 dark:text-blue-400" />
                  </div>
                  <div>
                    <div className="text-2xl font-bold">{deviceType.commands?.length || 0}</div>
                    <div className="text-xs text-muted-foreground">Commands</div>
                  </div>
                </div>
              </Card>
              <Card className="p-4">
                <div className="flex items-center gap-3">
                  <div className="p-2 rounded-lg bg-purple-100 dark:bg-purple-900/30">
                    <Settings className="h-5 w-5 text-purple-600 dark:text-purple-400" />
                  </div>
                  <div>
                    <div className="text-lg font-bold">{isRawMode ? 'Raw' : 'Full'}</div>
                    <div className="text-xs text-muted-foreground">Data Mode</div>
                  </div>
                </div>
              </Card>
            </div>

            {/* Description */}
            {deviceType.description && (
              <Card className="p-4">
                <p className="text-sm text-muted-foreground">{deviceType.description}</p>
              </Card>
            )}

            {/* Categories */}
            {deviceType.categories && deviceType.categories.length > 0 && (
              <div className="flex items-center gap-2 flex-wrap">
                <span className="text-sm text-muted-foreground">Categories:</span>
                {deviceType.categories.map((cat, i) => (
                  <Badge key={i} variant="outline">{cat}</Badge>
                ))}
              </div>
            )}

            {/* Metrics */}
            <Card className="p-4">
              <div className="flex items-center justify-between mb-4">
                <h4 className="font-medium flex items-center gap-2">
                  <ArrowDown className="h-4 w-4 text-green-500" />
                  Metrics ({deviceType.metrics?.length || 0})
                </h4>
                {isRawMode && (
                  <Badge variant="secondary" className="text-xs">
                    <Zap className="h-3 w-3 mr-1" />
                    Raw Data Mode
                  </Badge>
                )}
              </div>

              {(!deviceType.metrics || deviceType.metrics.length === 0) ? (
                <div className="text-center py-8 text-muted-foreground">
                  {isRawMode ? 'Payloads stored as-is without parsing' : 'No metrics defined'}
                </div>
              ) : (
                <div className="space-y-2">
                  {deviceType.metrics.map((metric, i) => (
                    <div key={i} className="p-3 bg-muted/50 rounded-lg">
                      <div className="flex items-center justify-between">
                        <div className="flex items-center gap-2">
                          <span className="font-mono text-sm">{metric.name}</span>
                          <span className="text-muted-foreground">•</span>
                          <span className="text-sm">{metric.display_name}</span>
                        </div>
                        <div className="flex items-center gap-2">
                          <Badge variant="outline" className="text-xs">{metric.data_type}</Badge>
                          {metric.unit && (
                            <span className="text-xs text-muted-foreground">({metric.unit})</span>
                          )}
                        </div>
                      </div>
                    </div>
                  ))}
                </div>
              )}
            </Card>

            {/* Commands */}
            <Card className="p-4">
              <div className="flex items-center justify-between mb-4">
                <h4 className="font-medium flex items-center gap-2">
                  <FileText className="h-4 w-4 text-blue-500" />
                  Commands ({deviceType.commands?.length || 0})
                </h4>
              </div>

              {(!deviceType.commands || deviceType.commands.length === 0) ? (
                <div className="text-center py-8 text-muted-foreground">
                  No commands defined
                </div>
              ) : (
                <div className="space-y-2">
                  {deviceType.commands.map((cmd, i) => (
                    <div key={i} className="p-3 bg-muted/50 rounded-lg">
                      <div className="flex items-center justify-between">
                        <div className="flex items-center gap-2">
                          <span className="font-mono text-sm">{cmd.name}</span>
                          <span className="text-muted-foreground">•</span>
                          <span className="text-sm">{cmd.display_name}</span>
                        </div>
                        <Badge variant="secondary" className="text-xs">
                          {cmd.parameters.length} params
                        </Badge>
                      </div>
                    </div>
                  ))}
                </div>
              )}
            </Card>

            {/* Sample Data */}
            {deviceType.uplink_samples && deviceType.uplink_samples.length > 0 && (
              <Card className="p-4">
                <h4 className="font-medium flex items-center gap-2 mb-3">
                  <Database className="h-4 w-4 text-orange-500" />
                  Sample Data ({deviceType.uplink_samples.length})
                </h4>
                <div className="space-y-2">
                  {deviceType.uplink_samples.map((sample, i) => (
                    <pre key={i} className="text-xs bg-muted p-3 rounded overflow-x-auto">
                      {JSON.stringify(sample, null, 2)}
                    </pre>
                  ))}
                </div>
              </Card>
            )}
          </div>
        </div>
      </DialogContent>
    </Dialog>
  )
}

// ============================================================================
// EDIT DEVICE TYPE DIALOG (wrapper around AddDeviceTypeDialog)
// ============================================================================

interface EditDeviceTypeDialogProps {
  open: boolean
  onOpenChange: (open: boolean) => void
  deviceType: DeviceType | null
  onEdit: (data: DeviceType) => Promise<boolean>
  editing: boolean
  onGenerateMDL?: (name: string, description: string, uplinkSample: string, downlinkSample: string) => Promise<string>
}

// Reuse AddDeviceTypeDialog with editDeviceType prop
export function EditDeviceTypeDialog({ open, onOpenChange, deviceType, onEdit, editing, onGenerateMDL }: EditDeviceTypeDialogProps) {
  // Default no-op validator for edit mode
  const handleValidate = async (): Promise<ValidationResult> => ({
    valid: true,
    message: "Ready to save"
  })

  // Default MDL generator if not provided
  const defaultGenerateMDL = async (): Promise<string> => {
    return JSON.stringify({ metrics: [], commands: [] })
  }

  const effectiveGenerateMDL = onGenerateMDL || defaultGenerateMDL

  return (
    <AddDeviceTypeDialog
      open={open}
      onOpenChange={onOpenChange}
      onAdd={onEdit}
      onValidate={handleValidate}
      onGenerateMDL={effectiveGenerateMDL}
      adding={editing}
      validating={false}
      generating={false}
      editDeviceType={deviceType}
    />
  )
}
