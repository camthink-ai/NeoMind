import React, { useState, useEffect } from "react"
import { useTranslation } from "react-i18next"
import { createPortal } from "react-dom"
import { Dialog, DialogContent, DialogHeader, DialogTitle, DialogDescription, DialogContentBody, DialogFooter } from "@/components/ui/dialog"
import { Button } from "@/components/ui/button"
import { Input } from "@/components/ui/input"
import { Label } from "@/components/ui/label"
import { Textarea } from "@/components/ui/textarea"
import { Badge } from "@/components/ui/badge"
import { Card } from "@/components/ui/card"
import { Switch } from "@/components/ui/switch"
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select"
import { useToast } from "@/components/ui/use-toast"
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
  DropdownMenuSeparator,
} from "@/components/ui/dropdown-menu"
import {
  Plus,
  Trash2,
  ChevronLeft,
  ChevronRight,
  ChevronDown,
  Check,
  X,
  AlertCircle,
  Sparkles,
  FileText,
  ArrowDown,
  Settings,
  Zap,
  Code,
  Database,
  MoreVertical,
  Github,
  Download,
  Loader2,
} from "lucide-react"
import { cn } from "@/lib/utils"
import { api, fetchAPI } from "@/lib/api"
import { useBodyScrollLock } from "@/hooks/useBodyScrollLock"
import { useIsMobile, useSafeAreaInsets } from "@/hooks/useMobile"
import type { DeviceType, MetricDefinition, CommandDefinition } from "@/types"

/**
 * Helper function to safely format data_type for display
 * Handles both string types and object formats from the API
 */
function formatDataType(dataType: string | Record<string, unknown> | undefined): string {
  if (!dataType) return 'unknown'
  if (typeof dataType === 'string') return dataType
  // If data_type is an object (e.g., { array: true }), format it
  if (typeof dataType === 'object') {
    const keys = Object.keys(dataType)
    if (keys.length === 1) {
      return keys[0]  // e.g., "array"
    }
    return JSON.stringify(dataType)
  }
  return String(dataType)
}

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
  adding: boolean
  validating: boolean
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
  adding,
  validating,
  editDeviceType,
}: AddDeviceTypeDialogProps) {
  const { t } = useTranslation(['common', 'devices'])
  const { toast } = useToast()
  const isEditMode = !!editDeviceType
  const isMobile = useIsMobile()
  const insets = useSafeAreaInsets()

  // Lock body scroll when dialog is open
  useBodyScrollLock(open)

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

  // Reset when dialog opens or editDeviceType changes
  useEffect(() => {
    if (open) {
      setCurrentStep('basic')
      setCompletedSteps(new Set())

      if (editDeviceType) {
        // Load existing data for edit mode
        setFormData(editDeviceType)
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
      }

      setFormErrors({})
      setValidationResult(null)
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
      const prevStep = steps[currentIndex - 1]
      setCurrentStep(prevStep)
      // Clear completed steps that come after the previous step
      // This ensures the completion state is accurate when navigating back
      const newCompleted = new Set<Step>()
      for (let i = 0; i < currentIndex - 1; i++) {
        newCompleted.add(steps[i])
      }
      setCompletedSteps(newCompleted)
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
  const steps: { key: Step; label: string; shortLabel: string; icon: React.ReactNode }[] = [
    { key: 'basic', label: 'Basic Info', shortLabel: 'Basic', icon: <Settings className="h-4 w-4" /> },
    { key: 'data', label: 'Data Definition', shortLabel: 'Data', icon: <ArrowDown className="h-4 w-4" /> },
    { key: 'commands', label: 'Commands', shortLabel: 'Commands', icon: <FileText className="h-4 w-4" /> },
    { key: 'review', label: 'Review', shortLabel: 'Review', icon: <Check className="h-4 w-4" /> },
    { key: 'finish', label: 'Finish', shortLabel: 'Finish', icon: <Sparkles className="h-4 w-4" /> },
  ]

  const stepIndex = steps.findIndex(s => s.key === currentStep)
  const isFirstStep = currentStep === 'basic'

  // Dialog content layout
  const content = (
    <>
      {/* Header */}
      <header
        className="border-b shrink-0 bg-background"
        style={isMobile ? { paddingTop: `${insets.top}px` } : undefined}
      >
        <div className={cn(
          "flex items-center gap-3",
          isMobile ? "px-4 py-4" : "px-4 py-3"
        )}>
          <Button
            variant="ghost"
            size="icon"
            className={cn("shrink-0", isMobile ? "h-10 w-10" : "h-8 w-8")}
            onClick={() => onOpenChange(false)}
          >
            <X className={cn(isMobile ? "h-5 w-5" : "h-4 w-4")} />
          </Button>
          <div className="flex items-center gap-2 min-w-0 flex-1">
            <div className={cn(
              "rounded-lg bg-amber-500/10 flex items-center justify-center shrink-0",
              isMobile ? "w-8 h-8" : "w-7 h-7"
            )}>
              <Zap className={cn(isMobile ? "h-4 w-4" : "h-3.5 w-3.5", "text-amber-500")} />
            </div>
            <h1 className={cn(
              "font-medium truncate",
              isMobile ? "text-base" : "text-sm"
            )}>
              {isEditMode ? 'Edit Device Type' : t('devices:types.add.title')}
            </h1>
          </div>
        </div>
      </header>

      {/* Content Area - Hide left sidebar on mobile */}
      <div className="flex flex-1 overflow-hidden">
        {/* Left Sidebar - Vertical Steps (Compact) - Hide on mobile */}
        {!isMobile && (
          <aside className="border-r shrink-0 bg-muted/20 w-[120px]">
            <nav className="px-3 py-6 space-y-1">
              {steps.map((step, index) => {
                const isCompleted = completedSteps.has(step.key)
                const isCurrent = step.key === currentStep
                const isPast = index < stepIndex

                return (
                  <div key={step.key} className="relative">
                    <button
                      onClick={() => {
                        if (isCompleted || isPast) {
                          setCurrentStep(step.key)
                        }
                      }}
                      className={cn(
                        "w-full text-left px-2 py-2 rounded-md transition-all flex flex-col items-center gap-1.5",
                        isCurrent && "bg-background shadow-sm",
                        !isCurrent && isPast && "hover:bg-background/50 cursor-pointer",
                        !isCurrent && !isPast && "opacity-50"
                      )}
                      title={step.label}
                    >
                      <div className="flex items-center justify-center">
                        {isCompleted ? (
                          <div className="w-5 h-5 rounded-full bg-green-500 flex items-center justify-center">
                            <Check className="h-3 w-3 text-white" />
                          </div>
                        ) : isCurrent ? (
                          <div className="w-5 h-5 rounded-full bg-primary flex items-center justify-center ring-4 ring-primary/20">
                            <span className="text-[10px] font-medium text-primary-foreground">{index + 1}</span>
                          </div>
                        ) : (
                          <div className="w-5 h-5 rounded-full bg-muted-foreground/20 flex items-center justify-center">
                            <span className="text-[10px] font-medium text-muted-foreground">{index + 1}</span>
                          </div>
                        )}
                      </div>
                      <div className="text-[10px] font-medium text-center leading-tight">
                        {step.shortLabel}
                      </div>
                    </button>

                    {/* Connector line to next step */}
                    {index < steps.length - 1 && (
                      <div className="absolute left-[23px] top-8 h-4 w-px">
                        <div className={cn(
                          "h-full w-px",
                          isPast ? "bg-primary" : "bg-border"
                        )} />
                      </div>
                    )}
                  </div>
                )
              })}
            </nav>
          </aside>
        )}

        {/* Main Content */}
        <main className="flex-1 overflow-y-auto">
          <div className={cn(
            isMobile ? "px-4 py-4" : "px-4 py-6"
          )}>
            <div className="space-y-4">
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
                  onChange={updateField as (field: keyof DeviceType, value: unknown) => void}
                  errors={formErrors}
                />
              )}

              {currentStep === 'commands' && (
                <CommandsStep
                  data={formData}
                  onChange={setFormData}
                  errors={formErrors}
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
          </div>
        </main>
      </div>

      {/* Footer Navigation */}
      {currentStep !== 'finish' && (
        <footer className={cn(
          "flex flex-row justify-end gap-2 sm:gap-3 border-t bg-background shrink-0",
          isMobile ? "px-4 py-3" : "px-6 py-4"
        )}>
          <div className="flex gap-2 w-full sm:w-auto justify-between sm:justify-end">
            {!isFirstStep && (
              <Button variant="outline" size={isMobile ? "default" : "sm"} onClick={handlePrevious} className={isMobile ? "h-12 min-w-[100px]" : ""}>
                <ChevronLeft className={cn(isMobile ? "h-4 w-4" : "h-3.5 w-3.5", "mr-1")} />
                {t('common:previous')}
              </Button>
            )}

            <div className="flex-1 sm:flex-1" />

            {currentStep === 'review' ? (
              <Button variant="outline" size={isMobile ? "default" : "sm"} onClick={handleSave} disabled={adding} className={isMobile ? "h-12 min-w-[100px]" : ""}>
                {adding ? (isEditMode ? 'Saving...' : t('devices:types.adding')) : (isEditMode ? 'Save' : t('common:save'))}
              </Button>
            ) : (
              <Button size={isMobile ? "default" : "sm"} onClick={handleNext} className={isMobile ? "h-12 min-w-[100px]" : ""}>
                {t('common:next')}
                <ChevronRight className={cn(isMobile ? "h-4 w-4" : "h-3.5 w-3.5", "ml-1")} />
              </Button>
            )}
          </div>
        </footer>
      )}
    </>
  )

  // Get portal root
  const dialogRoot = typeof document !== 'undefined'
    ? document.getElementById('portal-root') || document.body
    : null

  if (!dialogRoot) return null

  return createPortal(
    <div
      className={cn(
        "fixed inset-0 z-[100] bg-background flex flex-col",
        !open && "hidden"
      )}
    >
      {/* Overlay */}
      <div
        className={cn(
          "fixed inset-0 bg-black/80 transition-opacity",
          open ? "opacity-100" : "opacity-0 pointer-events-none"
        )}
        onClick={() => onOpenChange(false)}
      />

      {/* Main Content */}
      <div className={cn(
        "fixed bg-background shadow-lg flex flex-col overflow-hidden",
        isMobile
          ? "inset-0 rounded-none"
          : "left-[50%] top-[50%] -translate-x-1/2 -translate-y-1/2 w-full max-w-4xl h-[90vh] max-h-[90vh] rounded-lg border"
      )}>
        {content}
      </div>
    </div>,
    dialogRoot
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
  const isMobile = useIsMobile()
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
    <div className="flex flex-col h-full py-4">
      {!isMobile && (
        <div className="text-center mb-6 shrink-0">
          <h3 className="text-lg font-semibold">Basic Information</h3>
          <p className="text-sm text-muted-foreground">Enter the basic information for your device type</p>
        </div>
      )}

      <div className="flex-1 overflow-y-auto">
        <div className="space-y-6 max-w-2xl mx-auto">

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
      </div>
    </div>
  )
}

// ============================================================================
// STEP 2: Data Definition
// ============================================================================

interface DataDefinitionStepProps {
  data: Partial<DeviceType>
  onChange: (field: keyof DeviceType, value: unknown) => void
  errors: FormErrors
}

function DataDefinitionStep({
  data,
  onChange,
  errors,
}: DataDefinitionStepProps) {
  const { t } = useTranslation(['devices'])
  const { toast } = useToast()
  const isMobile = useIsMobile()
  const isRawMode = data.mode === 'simple'
  const [showImportDialog, setShowImportDialog] = useState(false)
  const [jsonInput, setJsonInput] = useState('')
  const [importError, setImportError] = useState('')

  // Add metric
  const addMetric = () => {
    const metrics = data.metrics || []
    // Find the highest numbered metric_ and increment
    let maxNum = 0
    for (const m of metrics) {
      const match = m.name.match(/^metric_(\d+)$/)
      if (match) {
        const num = parseInt(match[1], 10)
        if (num > maxNum) maxNum = num
      }
    }
    const newNum = maxNum + 1
    onChange('metrics', [
      ...metrics,
      {
        name: `metric_${newNum}`,
        display_name: `Metric ${newNum}`,
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

  // Infer data type from value
  const inferDataType = (value: unknown): 'string' | 'integer' | 'float' | 'boolean' => {
    if (typeof value === 'boolean') return 'boolean'
    if (typeof value === 'number') {
      return Number.isInteger(value) ? 'integer' : 'float'
    }
    if (typeof value === 'string') {
      // Try to parse as number
      if (/^-?\d+$/.test(value)) return 'integer'
      if (/^-?\d+\.\d+$/.test(value)) return 'float'
      if (value === 'true' || value === 'false') return 'boolean'
    }
    return 'string'
  }

  // Convert camelCase to snake_case
  const toSnakeCase = (str: string): string => {
    return str.replace(/[A-Z]/g, letter => `_${letter.toLowerCase()}`)
  }

  // Flatten nested JSON object into dot-notation paths
  // Supports up to 10 levels of nesting, including arrays of objects
  const flattenJson = (value: unknown, prefix: string = '', maxDepth: number = 10): Map<string, unknown> => {
    const result = new Map<string, unknown>()

    if (maxDepth <= 0) {
      return result
    }

    if (typeof value === 'object' && value !== null && !Array.isArray(value)) {
      // Object: recurse with dot notation
      for (const [key, val] of Object.entries(value)) {
        const newKey = prefix ? `${prefix}.${key}` : key

        if (typeof val === 'object' && val !== null) {
          if (Array.isArray(val)) {
            // Check if array contains objects/arrays
            const mayContainObjects = val.some(v =>
              typeof v === 'object' && v !== null && (Array.isArray(v) || Object.keys(v).length > 0)
            )
            if (mayContainObjects) {
              result.set(newKey, val) // Keep array as-is for complex arrays
              const flattened = flattenJson(val, newKey, maxDepth - 1)
              flattened.forEach((v, k) => result.set(k, v))
            } else {
              result.set(newKey, val) // Simple array of primitives
            }
          } else {
            // Nested object - flatten it
            const flattened = flattenJson(val, newKey, maxDepth - 1)
            flattened.forEach((v, k) => result.set(k, v))
          }
        } else {
          // Primitive value
          result.set(newKey, val)
        }
      }
    } else if (Array.isArray(value)) {
      // Array: expand with index notation
      for (let i = 0; i < value.length; i++) {
        const val = value[i]
        const newKey = prefix ? `${prefix}.${i}` : String(i)

        if (typeof val === 'object' && val !== null) {
          if (Array.isArray(val)) {
            const mayContainObjects = val.some(v =>
              typeof v === 'object' && v !== null && (Array.isArray(v) || Object.keys(v).length > 0)
            )
            if (mayContainObjects) {
              result.set(newKey, val)
              const flattened = flattenJson(val, newKey, maxDepth - 1)
              flattened.forEach((v, k) => result.set(k, v))
            } else {
              result.set(newKey, val)
            }
          } else {
            const flattened = flattenJson(val, newKey, maxDepth - 1)
            flattened.forEach((v, k) => result.set(k, v))
          }
        } else {
          result.set(newKey, val)
        }
      }
    } else if (prefix) {
      // Primitive value at root level
      result.set(prefix, value)
    }

    return result
  }

  // Parse JSON and generate metrics
  const parseJsonToMetrics = () => {
    setImportError('')
    try {
      const parsed = JSON.parse(jsonInput)
      const metricsToAdd: MetricDefinition[] = []

      // Handle array of samples
      const samples = Array.isArray(parsed) ? parsed : [parsed]

      // Collect all unique keys and their values using flattenJson for nested support
      const valueMap = new Map<string, Set<any>>()
      for (const sample of samples) {
        if (typeof sample === 'object' && sample !== null) {
          // Use flattenJson to get all nested paths
          const flattened = flattenJson(sample)
          for (const [key, value] of flattened.entries()) {
            // Skip array indices (paths ending with numbers like "data.0")
            // We only want the actual field paths, not individual array elements
            if (/^\d+$/.test(key)) continue
            // Skip paths that are just array indices at any level
            if (/\.\d+$/.test(key)) continue

            if (!valueMap.has(key)) {
              valueMap.set(key, new Set())
            }
            // Skip non-primitive values (objects, arrays) for type inference
            if (typeof value !== 'object' || value === null) {
              valueMap.get(key)!.add(value)
            }
          }
        }
      }

      // Generate metrics from collected data
      for (const [key, values] of valueMap.entries()) {
        if (values.size === 0) continue

        const sampleValue = Array.from(values)[0]
        const dataType = inferDataType(sampleValue)

        // Generate display name from key
        // For nested paths like "sensor.temperature", use "Temperature" or "Sensor Temperature"
        const parts = key.split('.')
        const lastPart = parts[parts.length - 1]
        const displayName = lastPart
          .replace(/_/g, ' ')
          .replace(/([A-Z])/g, ' $1')
          .replace(/^\s/, '')
          .replace(/^\w/, c => c.toUpperCase())

        // For deeply nested paths, add prefix for clarity
        const finalDisplayName = parts.length > 1
          ? `${parts[parts.length - 2].replace(/_/g, ' ').replace(/^\w/, c => c.toUpperCase())} ${displayName}`
          : displayName

        // Detect if this looks like an enum
        const valueArray = Array.from(values)
        let allowedValues: string[] | undefined
        if (values.size > 1 && values.size <= 10) {
          // Check if all values are strings and limited count -> might be enum
          if (valueArray.every(v => typeof v === 'string')) {
            allowedValues = valueArray as string[]
          }
        }

        metricsToAdd.push({
          // Convert dotted path to snake_case (e.g., "sensor.temperature" -> "sensor_temperature")
          name: key.replace(/\./g, '_'),
          display_name: finalDisplayName,
          data_type: dataType,
          unit: '',
          ...(allowedValues && { allowed_values: allowedValues }),
        })
      }

      onChange('metrics', [...(data.metrics || []), ...metricsToAdd])
      setShowImportDialog(false)
      setJsonInput('')
      toast({
        title: t('devices:metricEditor.importSuccess'),
        description: t('devices:metricEditor.importSuccessDesc', { count: metricsToAdd.length }),
      })
    } catch (e) {
      setImportError(t('devices:metricEditor.jsonError'))
    }
  }

  return (
    <div className="flex flex-col h-full py-4">
      {!isMobile && (
        <div className="text-center mb-4 shrink-0">
          <h3 className="text-lg font-semibold">Data Definition (Uplink)</h3>
          <p className="text-sm text-muted-foreground">Define how device data is parsed and stored</p>
        </div>
      )}

      <div className="flex-1 overflow-y-auto min-h-0">
        <div className="space-y-4">
          {/* Mode Selection */}
          <div className="flex justify-center gap-4 shrink-0">
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
        <div className="space-y-4">
          {/* Quick Start */}
          {(!data.metrics || data.metrics.length === 0) && (
            <div className="p-4 bg-blue-50 dark:bg-blue-950/20 rounded-lg border border-blue-200 dark:border-blue-800">
              <p className="text-xs font-medium text-blue-700 dark:text-blue-300 mb-2">
                {t('devices:metricEditor.quickStart')}
              </p>
              <p className="text-xs text-blue-600 dark:text-blue-400 mb-3">
                {t('devices:metricEditor.quickStartDesc')}
              </p>
              <div className="flex gap-2">
                <Button
                  variant="outline"
                  size="sm"
                  onClick={() => setShowImportDialog(true)}
                  className="h-8 text-xs"
                >
                  <Code className="mr-1 h-3 w-3" />
                  {t('devices:metricEditor.generateFromJson')}
                </Button>
              </div>
            </div>
          )}

          {/* Manual Entry List */}
          <div className="space-y-3">
            <div className="flex items-center justify-between">
              <h4 className="text-sm font-medium">
                {t('devices:metricEditor.metricCount', { count: data.metrics?.length || 0 })}
              </h4>
              <div className="flex items-center gap-2">
                <Button
                  variant="outline"
                  size="sm"
                  onClick={() => setShowImportDialog(true)}
                  className="gap-1 h-8"
                >
                  <Code className="h-3 w-3" />
                  {t('devices:metricEditor.importJson')}
                </Button>
                <Button onClick={addMetric} size="sm" variant="outline" className="h-8">
                  <Plus className="mr-1 h-3 w-3" />
                  {t('devices:metricEditor.addMetric')}
                </Button>
              </div>
            </div>

            {(!data.metrics || data.metrics.length === 0) ? (
              <div className="flex items-center justify-center border-2 border-dashed rounded-lg bg-muted/20 py-12">
                <div className="text-center">
                  <FileText className="h-8 w-8 mx-auto mb-2 text-muted-foreground" />
                  <p className="text-sm text-muted-foreground">No metrics defined</p>
                  <p className="text-xs text-muted-foreground mt-1">
                    {t('devices:metricEditor.orImportFromJson')}
                  </p>
                </div>
              </div>
            ) : (
              <div className="space-y-2">
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
              <Database className="h-6 w-6 text-muted-foreground" />
            </div>
            <h4 className="font-medium mb-2">Raw Data Mode</h4>
            <p className="text-sm text-muted-foreground mb-2">
              {t('devices:metricEditor.simpleModeDesc')}
            </p>
            <p className="text-xs text-muted-foreground">
              {t('devices:metricEditor.simpleModeNote')}
            </p>
          </div>
        </div>
      )}
        </div>
      </div>

      {/* JSON Import Dialog */}
      <Dialog open={showImportDialog} onOpenChange={setShowImportDialog}>
        <DialogContent className="sm:max-w-lg">
          <DialogHeader>
            <DialogTitle>{t('devices:metricEditor.jsonDialogTitle')}</DialogTitle>
            <DialogDescription>
              {t('devices:metricEditor.jsonDialogDesc')}
            </DialogDescription>
          </DialogHeader>
          <DialogContentBody className="space-y-4 py-4">
            <Textarea
              value={jsonInput}
              onChange={(e) => setJsonInput(e.target.value)}
              placeholder={t('devices:metricEditor.jsonPlaceholder')}
              className="font-mono text-xs min-h-[150px] resize-none"
            />
            {importError && (
              <p className="text-xs text-destructive flex items-center gap-1">
                <AlertCircle className="h-3 w-3" />
                {importError}
              </p>
            )}
            <div className="p-3 bg-muted/50 rounded text-xs">
              <p className="font-medium mb-1">{t('devices:metricEditor.exampleJson')}</p>
              <pre className="text-muted-foreground">{`{
  "sensor": {
    "temperature": 25.5,
    "humidity": 60
  },
  "status": "online",
  "battery": 85
}`}</pre>
            </div>
          </DialogContentBody>
          <DialogFooter>
            <Button variant="outline" onClick={() => setShowImportDialog(false)}>
              {t('devices:metricEditor.cancel')}
            </Button>
            <Button onClick={parseJsonToMetrics}>
              {t('devices:metricEditor.parseAndGenerate')}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
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
}

function CommandsStep({
  data,
  onChange,
  errors,
}: CommandsStepProps) {
  const { t } = useTranslation(['devices'])
  const { toast } = useToast()
  const isMobile = useIsMobile()

  // Add command
  const addCommand = () => {
    const commands = data.commands || []
    // Find the highest numbered cmd_ and increment
    let maxNum = 0
    for (const cmd of commands) {
      const match = cmd.name.match(/^cmd_(\d+)$/)
      if (match) {
        const num = parseInt(match[1], 10)
        if (num > maxNum) maxNum = num
      }
    }
    const newNum = maxNum + 1
    onChange({
      ...data,
      commands: [
        ...commands,
        {
          name: `cmd_${newNum}`,
          display_name: `Command ${newNum}`,
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

  // Import from JSON
  const importFromJson = () => {
    const jsonInput = prompt(t('devices:commandsStep.promptJson', 'Paste JSON to import commands:'))
    if (!jsonInput) return

    try {
      const imported = JSON.parse(jsonInput)
      const commandsToAdd = Array.isArray(imported) ? imported : [imported]

      // Convert to CommandDefinition format
      const newCommands = commandsToAdd.map((cmd: any) => ({
        name: cmd.name || `cmd_${Date.now()}`,
        display_name: cmd.display_name || cmd.name || t('devices:commandsStep.importedCommand', 'Imported Command'),
        payload_template: cmd.payload_template || cmd.payload || JSON.stringify(cmd),
        parameters: cmd.parameters || [],
      }))

      onChange({
        ...data,
        commands: [...(data.commands || []), ...newCommands],
      })

      toast({
        title: t('devices:commandsStep.importSuccess', 'Import Successful'),
        description: t('devices:commandsStep.importSuccessDesc', `Added ${newCommands.length} command${newCommands.length > 1 ? 's' : ''}`, { count: newCommands.length }),
      })
    } catch {
      toast({
        title: t('devices:commandsStep.importFailed', 'Import Failed'),
        description: t('devices:commandsStep.invalidJson', 'Invalid JSON format'),
        variant: 'destructive',
      })
    }
  }

  // Hidden file input for JSON import
  const fileInputRef = React.useRef<HTMLInputElement>(null)

  const handleFileImport = (e: React.ChangeEvent<HTMLInputElement>) => {
    const file = e.target.files?.[0]
    if (!file) return

    const reader = new FileReader()
    reader.onload = (event) => {
      try {
        const imported = JSON.parse(event.target?.result as string)
        const commandsToAdd = Array.isArray(imported) ? imported : [imported]

        const newCommands = commandsToAdd.map((cmd: any) => ({
          name: cmd.name || `cmd_${Date.now()}`,
          display_name: cmd.display_name || cmd.name || t('devices:commandsStep.importedCommand', 'Imported Command'),
          payload_template: cmd.payload_template || cmd.payload || JSON.stringify(cmd),
          parameters: cmd.parameters || [],
        }))

        onChange({
          ...data,
          commands: [...(data.commands || []), ...newCommands],
        })

        toast({
          title: t('devices:commandsStep.importSuccess', 'Import Successful'),
          description: t('devices:commandsStep.importSuccessDesc', `Added ${newCommands.length} command${newCommands.length > 1 ? 's' : ''}`, { count: newCommands.length }),
        })
      } catch {
        toast({
          title: t('devices:commandsStep.importFailed', 'Import Failed'),
          description: t('devices:commandsStep.invalidJson', 'Invalid JSON format'),
          variant: 'destructive',
        })
      } finally {
        if (fileInputRef.current) {
          fileInputRef.current.value = ''
        }
      }
    }
    reader.readAsText(file)
  }

  return (
    <div className="flex flex-col h-full py-4">
      {!isMobile && (
        <div className="text-center mb-4 shrink-0">
          <h3 className="text-lg font-semibold">Commands (Downlink)</h3>
          <p className="text-sm text-muted-foreground">Define commands that can be sent to the device</p>
        </div>
      )}

      <div className="flex-1 overflow-y-auto min-h-0">
        <div className="space-y-4">
          {/* Manual Entry List */}
          <div className="space-y-3">
            <div className="flex items-center justify-between">
              <h4 className="text-sm font-medium flex items-center gap-2">
                <FileText className="h-4 w-4" />
                Commands ({data.commands?.length || 0})
              </h4>
              <DropdownMenu>
                <DropdownMenuTrigger asChild>
                  <Button size="sm" variant="outline" className="h-8">
                    <Plus className="mr-1 h-3 w-3" />
                    Add Command
                    <MoreVertical className="ml-1 h-3 w-3" />
                  </Button>
                </DropdownMenuTrigger>
                <DropdownMenuContent align="end">
                  <DropdownMenuItem onClick={addCommand}>
                    <Plus className="mr-2 h-3 w-3" />
                    Empty Command
                  </DropdownMenuItem>
                  <DropdownMenuSeparator />
                  <DropdownMenuItem onClick={importFromJson}>
                    <Code className="mr-2 h-3 w-3" />
                    {t('devices:commandsStep.importJson', 'Import from JSON')}
                  </DropdownMenuItem>
                  <DropdownMenuItem onClick={() => fileInputRef.current?.click()}>
                    <Database className="mr-2 h-3 w-3" />
                    {t('devices:commandsStep.importFromFile', 'Import from File')}
                  </DropdownMenuItem>
                </DropdownMenuContent>
              </DropdownMenu>
              <input
                ref={fileInputRef}
                type="file"
                accept=".json"
                className="hidden"
                onChange={handleFileImport}
              />
            </div>

            {(!data.commands || data.commands.length === 0) ? (
              <div className="text-center py-12 border-2 border-dashed rounded-lg bg-muted/20">
                <FileText className="h-10 w-10 mx-auto text-muted-foreground/50 mb-3" />
                <p className="text-sm text-muted-foreground">No commands defined</p>
                <p className="text-xs text-muted-foreground mt-1">
                  {t('devices:commandsStep.orImportFromJson', 'Add commands manually or import from JSON')}
                </p>
              </div>
            ) : (
              <div className="space-y-2">
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
  const isMobile = useIsMobile()
  const handleValidate = async () => {
    await onValidate()
  }

  return (
    <div className="flex flex-col h-full py-4">
      {!isMobile && (
        <div className="text-center mb-4 shrink-0">
          <h3 className="text-lg font-semibold">Review & Confirm</h3>
          <p className="text-sm text-muted-foreground">Review your device type before saving</p>
        </div>
      )}

      <div className="flex-1 overflow-y-auto min-h-0">
        <div className="space-y-6 max-w-3xl mx-auto">
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
            <div className="grid grid-cols-1 sm:grid-cols-2 gap-4 text-sm">
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
                      <Badge variant="outline" className="text-xs">{formatDataType(metric.data_type)}</Badge>
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
                      {cmd.parameters?.length || 0} params
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
  const { t } = useTranslation(['devices'])
  const [expanded, setExpanded] = useState(false)

  return (
    <div className={cn(
      "rounded-lg border p-3 space-y-2",
      error && "border-destructive"
    )}>
      <div className="flex items-center justify-between">
        <div className="flex items-center gap-2">
          <span className="font-mono text-sm">{metric.name}</span>
          <Badge variant="outline" className="text-xs">{formatDataType(metric.data_type)}</Badge>
          {metric.unit && <span className="text-xs text-muted-foreground">{metric.unit}</span>}
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
        <div className="space-y-3 pt-2 border-t">
          {/* First row: Display name and identifier */}
          <div className="grid grid-cols-1 sm:grid-cols-2 gap-3">
            <div className="space-y-1.5">
              <Label className="text-xs text-muted-foreground">{t('devices:metricEditor.displayName')}</Label>
              <Input
                value={metric.display_name}
                onChange={(e) => onChange({ ...metric, display_name: e.target.value })}
                placeholder={t('devices:metricEditor.displayNamePlaceholder')}
                className="h-9 text-sm"
              />
            </div>
            <div className="space-y-1.5">
              <Label className="text-xs text-muted-foreground">{t('devices:metricEditor.metricId')}</Label>
              <Input
                value={metric.name}
                onChange={(e) => onChange({ ...metric, name: e.target.value })}
                placeholder={t('devices:metricEditor.metricIdPlaceholder')}
                className="h-9 text-sm font-mono"
              />
            </div>
          </div>

          {/* Second row: Data type, unit, range */}
          <div className="grid grid-cols-[1fr_1fr_1.5fr] gap-3 items-end">
            <div className="space-y-1.5">
              <Label className="text-xs text-muted-foreground">{t('devices:metricEditor.dataType')}</Label>
              <Select
                value={metric.data_type}
                onValueChange={(value) => onChange({ ...metric, data_type: value as any })}
              >
                <SelectTrigger className="h-9 text-sm">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="string">{t('devices:metricEditor.types.string')}</SelectItem>
                  <SelectItem value="integer">{t('devices:metricEditor.types.integer')}</SelectItem>
                  <SelectItem value="float">{t('devices:metricEditor.types.float')}</SelectItem>
                  <SelectItem value="boolean">{t('devices:metricEditor.types.boolean')}</SelectItem>
                  <SelectItem value="binary">{t('devices:metricEditor.types.binary')}</SelectItem>
                </SelectContent>
              </Select>
            </div>
            <div className="space-y-1.5">
              <Label className="text-xs text-muted-foreground">{t('devices:metricEditor.unit')}</Label>
              <Input
                value={metric.unit || ""}
                onChange={(e) => onChange({ ...metric, unit: e.target.value })}
                placeholder={t('devices:metricEditor.unitPlaceholder')}
                className="h-9 text-sm"
              />
            </div>
            <div className="space-y-1.5">
              <Label className="text-xs text-muted-foreground">{t('devices:metricEditor.minMax')}</Label>
              <div className="flex items-center gap-2">
                <Input
                  type="number"
                  value={metric.min ?? ''}
                  onChange={(e) => onChange({ ...metric, min: e.target.value ? parseFloat(e.target.value) : undefined })}
                  placeholder={t('devices:metricEditor.minPlaceholder')}
                  className="h-9 text-sm flex-1"
                />
                <span className="text-muted-foreground text-xs shrink-0">{t('devices:metricEditor.to')}</span>
                <Input
                  type="number"
                  value={metric.max ?? ''}
                  onChange={(e) => onChange({ ...metric, max: e.target.value ? parseFloat(e.target.value) : undefined })}
                  placeholder={t('devices:metricEditor.maxPlaceholder')}
                  className="h-9 text-sm flex-1"
                />
              </div>
            </div>
          </div>
        </div>
      )}
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
  const { t } = useTranslation(['devices'])
  const [expanded, setExpanded] = useState(false)
  const [showAdvanced, setShowAdvanced] = useState(false)
  const [previewMode, setPreviewMode] = useState<'visual' | 'json'>('visual')

  // Command templates for quick start
  const commandTemplates: { name: string; template: CommandDefinition }[] = [
    {
      name: t('devices:commandEditor.templates.toggle.name'),
      template: {
        name: 'toggle',
        display_name: t('devices:commandEditor.templates.toggle.displayName'),
        payload_template: '{"state": "${state}"}',
        parameters: [
          { name: 'state', display_name: t('devices:commandEditor.templates.toggle.stateParam'), data_type: 'string', allowed_values: ['on', 'off'], required: true }
        ]
      }
    },
    {
      name: t('devices:commandEditor.templates.setValue.name'),
      template: {
        name: 'set_value',
        display_name: t('devices:commandEditor.templates.setValue.displayName'),
        payload_template: '{"value": ${value}}',
        parameters: [
          { name: 'value', display_name: t('devices:commandEditor.templates.setValue.valueParam'), data_type: 'float', required: true }
        ]
      }
    },
    {
      name: t('devices:commandEditor.templates.setMode.name'),
      template: {
        name: 'set_mode',
        display_name: t('devices:commandEditor.templates.setMode.displayName'),
        payload_template: '{"mode": "${mode}"}',
        parameters: [
          { name: 'mode', display_name: t('devices:commandEditor.templates.setMode.modeParam'), data_type: 'string', required: true }
        ]
      }
    },
    {
      name: t('devices:commandEditor.templates.trigger.name'),
      template: {
        name: 'trigger',
        display_name: t('devices:commandEditor.templates.trigger.displayName'),
        payload_template: '{"action": "trigger"}',
        parameters: []
      }
    }
  ]

  // Add a new parameter
  const addParameter = () => {
    const params = command.parameters || []
    const newParam = {
      name: `param_${params.length + 1}`,
      display_name: t('devices:commandEditor.newParam', { count: params.length + 1 }),
      data_type: 'string' as const,
      required: false,
    }
    onChange({ ...command, parameters: [...params, newParam] })
  }

  // Update a parameter
  const updateParameter = (index: number, param: any) => {
    const params = command.parameters || []
    const newParams = [...params]
    newParams[index] = { ...newParams[index], ...param }
    onChange({ ...command, parameters: newParams })
  }

  // Remove a parameter
  const removeParameter = (index: number) => {
    const params = command.parameters || []
    onChange({ ...command, parameters: params.filter((_, i) => i !== index) })
  }

  // Add a fixed value
  const addFixedValue = () => {
    const fixed = command.fixed_values || {}
    onChange({ ...command, fixed_values: { ...fixed, [`fixed_${Object.keys(fixed).length + 1}`]: '' } })
  }

  // Update a fixed value
  const updateFixedValue = (key: string, value: unknown) => {
    const fixed = command.fixed_values || {}
    onChange({ ...command, fixed_values: { ...fixed, [key]: value } })
  }

  // Remove a fixed value
  const removeFixedValue = (key: string) => {
    const fixed = command.fixed_values || {}
    const newFixed = { ...fixed }
    delete newFixed[key]
    onChange({ ...command, fixed_values: Object.keys(newFixed).length > 0 ? newFixed : undefined })
  }

  // Generate preview of the payload
  const generatePreview = (): string => {
    let template = command.payload_template || '{}'
    // Replace fixed values with example values
    Object.entries(command.fixed_values || {}).forEach(([key, value]) => {
      const placeholder = `\${${key}}`
      const valueStr = typeof value === 'string' ? `"${value}"` : String(value)
      template = template.replace(placeholder, valueStr)
    })
    // Replace parameters with placeholder values
    command.parameters?.forEach(param => {
      const placeholder = `\${${param.name}}`
      let exampleValue = '...'
      if (param.allowed_values && param.allowed_values.length > 0) {
        exampleValue = typeof param.allowed_values[0] === 'string'
          ? `"${param.allowed_values[0]}"`
          : String(param.allowed_values[0])
      } else if (param.data_type === 'integer') {
        exampleValue = '0'
      } else if (param.data_type === 'float') {
        exampleValue = '0.0'
      } else if (param.data_type === 'boolean') {
        exampleValue = 'true'
      } else {
        exampleValue = `"${param.name}"`
      }
      template = template.replace(placeholder, exampleValue)
    })
    return template
  }

  // Format data type for display
  const formatDataType = (dt: string | Record<string, unknown>): string => {
    if (typeof dt === 'string') return dt
    if (typeof dt === 'object' && dt !== null) {
      if ('enum' in dt) return `enum: ${JSON.stringify((dt as any).enum)}`
      return JSON.stringify(dt)
    }
    return String(dt)
  }

  // Apply template
  const applyTemplate = (template: CommandDefinition) => {
    onChange({
      ...template,
      name: command.name, // Keep the original command name
      fixed_values: command.fixed_values, // Keep existing fixed values
    })
  }

  return (
    <div className={cn(
      "rounded-lg border p-3 space-y-2",
      error && "border-destructive"
    )}>
      <div className="flex items-center justify-between">
        <div className="flex items-center gap-2">
          <span className="font-mono text-sm">{command.name}</span>
          <Badge variant="secondary" className="text-xs">
            {t('devices:commandEditor.paramCount', { count: command.parameters?.length || 0 })}
          </Badge>
          {command.fixed_values && Object.keys(command.fixed_values).length > 0 && (
            <Badge variant="outline" className="text-xs text-green-600">
              {t('devices:commandEditor.fixedValueCount', { count: Object.keys(command.fixed_values).length })}
            </Badge>
          )}
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
        <div className="space-y-4 pt-2 border-t">
          {/* Quick Templates */}
          {(!command.parameters || command.parameters.length === 0) && !command.payload_template && (
            <div className="p-3 bg-blue-50 dark:bg-blue-950/20 rounded-lg border border-blue-200 dark:border-blue-800">
              <p className="text-xs font-medium text-blue-700 dark:text-blue-300 mb-2">
                {t('devices:commandEditor.quickStart')}
              </p>
              <div className="grid grid-cols-1 sm:grid-cols-2 gap-2">
                {commandTemplates.map((tmpl) => (
                  <Button
                    key={tmpl.name}
                    variant="outline"
                    size="sm"
                    onClick={() => applyTemplate(tmpl.template)}
                    className="h-7 text-xs justify-start"
                  >
                    {tmpl.name}
                  </Button>
                ))}
              </div>
            </div>
          )}

          {/* Basic fields */}
          <div className="space-y-2">
            <div className="grid grid-cols-1 sm:grid-cols-2 gap-2">
              <div className="space-y-1">
                <Label className="text-xs text-muted-foreground">{t('devices:commandEditor.displayName')}</Label>
                <Input
                  value={command.display_name}
                  onChange={(e) => onChange({ ...command, display_name: e.target.value })}
                  placeholder={t('devices:commandEditor.displayNamePlaceholder')}
                  className="h-8 text-sm"
                />
              </div>
              <div className="space-y-1">
                <Label className="text-xs text-muted-foreground">{t('devices:commandEditor.commandId')}</Label>
                <Input
                  value={command.name}
                  onChange={(e) => onChange({ ...command, name: e.target.value })}
                  placeholder={t('devices:commandEditor.commandIdPlaceholder')}
                  className="h-8 text-sm font-mono"
                />
              </div>
            </div>
          </div>

          {/* Payload Builder */}
          <div className="space-y-2">
            <div className="flex items-center justify-between">
              <Label className="text-xs font-medium">{t('devices:commandEditor.payloadFormat')}</Label>
              <div className="flex items-center gap-1">
                <Button
                  variant={previewMode === 'visual' ? 'secondary' : 'ghost'}
                  size="sm"
                  onClick={() => setPreviewMode('visual')}
                  className="h-6 text-xs"
                >
                  {t('devices:commandEditor.visual')}
                </Button>
                <Button
                  variant={previewMode === 'json' ? 'secondary' : 'ghost'}
                  size="sm"
                  onClick={() => setPreviewMode('json')}
                  className="h-6 text-xs"
                >
                  {t('devices:commandEditor.json')}
                </Button>
              </div>
            </div>

            {previewMode === 'visual' ? (
              <div className="p-3 bg-muted/30 rounded-lg border">
                <div className="font-mono text-xs">
                  {'{'}
                  {[
                    ...(Object.keys(command.fixed_values || {}).map(k => ({
                      key: k,
                      value: command.fixed_values![k],
                      type: 'fixed' as const
                    }))),
                    ...(command.parameters || []).map(p => ({
                      key: p.name,
                      value: `\${${p.name}}`,
                      type: 'param' as const,
                      param: p
                    }))
                  ].map((item, idx, arr) => (
                    <div key={item.key} className="ml-2">
                      <span className="text-blue-600">"{item.key}"</span>
                      <span className="text-muted-foreground">: </span>
                      {item.type === 'fixed' ? (
                        <span className="text-green-600">
                          {typeof item.value === 'string' ? `"${item.value}"` : JSON.stringify(item.value)}
                        </span>
                      ) : (
                        <span className="text-amber-600 bg-amber-100 dark:bg-amber-900/30 px-1 rounded">
                          {item.value}
                        </span>
                      )}
                      {idx < arr.length - 1 && <span className="text-muted-foreground">,</span>}
                      {item.type === 'param' && (
                        <div className="ml-4 text-xs text-muted-foreground">
                          {(item as any).param?.display_name || (item as any).param?.name}
                          {(item as any).param?.data_type !== 'string' && ` (${formatDataType((item as any).param?.data_type || 'string')})`}
                        </div>
                      )}
                    </div>
                  ))}
                  {'}'}
                </div>
                <p className="text-xs text-muted-foreground mt-2 italic">
                  {t('devices:commandEditor.previewHint')}
                </p>
              </div>
            ) : (
              <div className="space-y-2">
                <Textarea
                  value={command.payload_template || ''}
                  onChange={(e) => onChange({ ...command, payload_template: e.target.value })}
                  placeholder='{"action": "${value}"}'
                  className="h-16 text-sm font-mono text-xs resize-none"
                />
                <div className="flex items-center gap-2">
                  <span className="text-xs text-muted-foreground">{t('devices:commandEditor.preview')}</span>
                  <code className="text-xs bg-muted px-2 py-1 rounded font-mono">
                    {generatePreview()}
                  </code>
                </div>
              </div>
            )}
          </div>

          {/* Parameters Section */}
          <div className="space-y-2">
            <div className="flex items-center justify-between">
              <Label className="text-xs font-medium">
                {t('devices:commandEditor.userParams')}
                <span className="text-muted-foreground font-normal ml-1">
                  {t('devices:commandEditor.userParamsDesc')}
                </span>
              </Label>
              <Button
                variant="outline"
                size="sm"
                onClick={addParameter}
                className="h-7 text-xs"
              >
                <Plus className="mr-1 h-3 w-3" />
                {t('devices:commandEditor.addParam')}
              </Button>
            </div>

            {(!command.parameters || command.parameters.length === 0) ? (
              <div className="text-xs text-muted-foreground italic p-3 bg-muted/30 rounded text-center">
                {t('devices:commandEditor.noParams')}
              </div>
            ) : (
              <div className="border rounded-lg overflow-hidden">
                <table className="w-full text-xs">
                  <thead className="bg-muted">
                    <tr>
                      <th className="px-2 py-1 text-left font-medium">{t('devices:commandEditor.tableName')}</th>
                      <th className="px-2 py-1 text-left font-medium">{t('devices:commandEditor.tableDisplayName')}</th>
                      <th className="px-2 py-1 text-left font-medium">{t('devices:commandEditor.tableType')}</th>
                      <th className="px-2 py-1 text-center font-medium">{t('devices:commandEditor.tableRequired')}</th>
                      <th className="px-2 py-1 text-left font-medium">{t('devices:commandEditor.tableOptions')}</th>
                      <th className="px-2 py-1 w-8"></th>
                    </tr>
                  </thead>
                  <tbody>
                    {command.parameters.map((param, pIdx) => (
                      <tr key={pIdx} className="border-t">
                        <td className="px-2 py-1">
                          <Input
                            value={param.name}
                            onChange={(e) => updateParameter(pIdx, { name: e.target.value })}
                            placeholder={t('devices:commandEditor.paramNamePlaceholder')}
                            className="h-7 text-xs font-mono"
                          />
                        </td>
                        <td className="px-2 py-1">
                          <Input
                            value={param.display_name || ''}
                            onChange={(e) => updateParameter(pIdx, { display_name: e.target.value })}
                            placeholder={t('devices:commandEditor.displayNamePlaceholder')}
                            className="h-7 text-xs"
                          />
                        </td>
                        <td className="px-2 py-1">
                          <Select
                            value={typeof param.data_type === 'string' ? param.data_type : 'string'}
                            onValueChange={(value) => updateParameter(pIdx, { data_type: value })}
                          >
                            <SelectTrigger className="h-7 text-xs px-2">
                              <SelectValue />
                            </SelectTrigger>
                            <SelectContent>
                              <SelectItem value="string">{t('devices:commandEditor.types.string')}</SelectItem>
                              <SelectItem value="integer">{t('devices:commandEditor.types.integer')}</SelectItem>
                              <SelectItem value="float">{t('devices:commandEditor.types.float')}</SelectItem>
                              <SelectItem value="boolean">{t('devices:commandEditor.types.boolean')}</SelectItem>
                              <SelectItem value="binary">{t('devices:commandEditor.types.binary')}</SelectItem>
                              <SelectItem value="array">{t('devices:commandEditor.types.array')}</SelectItem>
                            </SelectContent>
                          </Select>
                        </td>
                        <td className="px-2 py-1 text-center">
                          <input
                            type="checkbox"
                            checked={param.required || false}
                            onChange={(e) => updateParameter(pIdx, { required: e.target.checked })}
                            className="h-3 w-3"
                          />
                        </td>
                        <td className="px-2 py-1">
                          {param.data_type === 'integer' || param.data_type === 'float' ? (
                            <div className="flex items-center gap-1">
                              <Input
                                type="number"
                                value={param.min ?? ''}
                                onChange={(e) => updateParameter(pIdx, { min: e.target.value ? parseFloat(e.target.value) : undefined })}
                                placeholder={t('devices:commandEditor.minValuePlaceholder')}
                                className="h-6 text-xs w-14"
                              />
                              <span className="text-muted-foreground">-</span>
                              <Input
                                type="number"
                                value={param.max ?? ''}
                                onChange={(e) => updateParameter(pIdx, { max: e.target.value ? parseFloat(e.target.value) : undefined })}
                                placeholder={t('devices:commandEditor.maxValuePlaceholder')}
                                className="h-6 text-xs w-14"
                              />
                            </div>
                          ) : param.data_type === 'boolean' ? (
                            <span className="text-muted-foreground text-xs">{t('devices:commandEditor.yesNo')}</span>
                          ) : (
                            <Input
                              value={(param.allowed_values || []).join(',')}
                              onChange={(e) => {
                                const values = e.target.value.split(',').map(v => v.trim()).filter(v => v)
                                updateParameter(pIdx, { allowed_values: values.length > 0 ? values : undefined })
                              }}
                              placeholder={t('devices:commandEditor.allowedValuesPlaceholder')}
                              className="h-6 text-xs"
                            />
                          )}
                        </td>
                        <td className="px-2 py-1">
                          <Button
                            variant="ghost"
                            size="icon"
                            onClick={() => removeParameter(pIdx)}
                            className="h-6 w-6"
                          >
                            <Trash2 className="h-3 w-3 text-destructive" />
                          </Button>
                        </td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>
            )}
          </div>

          {/* Fixed Values Section - Advanced */}
          <div className="space-y-2">
            <Button
              variant="ghost"
              size="sm"
              onClick={() => setShowAdvanced(!showAdvanced)}
              className="h-7 text-xs text-muted-foreground"
            >
              {showAdvanced ? t('devices:commandEditor.hideAdvanced') : t('devices:commandEditor.advancedOptions')}
            </Button>

            {showAdvanced && (
              <div className="p-3 bg-muted/30 rounded-lg space-y-2">
                <p className="text-xs text-muted-foreground">
                  {t('devices:commandEditor.fixedValuesDesc')}
                </p>
                <div className="flex items-center justify-between">
                  <span className="text-xs font-medium text-green-600">
                    {t('devices:commandEditor.fixedValues', { count: Object.keys(command.fixed_values || {}).length })}
                  </span>
                  <Button
                    variant="outline"
                    size="sm"
                    onClick={addFixedValue}
                    className="h-6 text-xs"
                  >
                    <Plus className="mr-1 h-3 w-3" />
                    {t('devices:commandEditor.add')}
                  </Button>
                </div>
                {Object.entries(command.fixed_values || {}).map(([key, value]) => (
                  <div key={key} className="flex items-center gap-1">
                    <Input
                      value={key}
                      onChange={(e) => {
                        const newFixed: Record<string, unknown> = {}
                        Object.entries(command.fixed_values || {}).forEach(([k, v]) => {
                          if (k === key) newFixed[e.target.value] = v
                          else newFixed[k] = v
                        })
                        onChange({ ...command, fixed_values: newFixed })
                      }}
                      placeholder={t('devices:commandEditor.keyPlaceholder')}
                      className="h-7 text-xs font-mono w-24"
                    />
                    <span className="text-muted-foreground text-xs">=</span>
                    <Input
                      value={String(value)}
                      onChange={(e) => updateFixedValue(key, e.target.value)}
                      placeholder={t('devices:commandEditor.valuePlaceholder')}
                      className="h-7 text-xs font-mono flex-1"
                    />
                    <Button
                      variant="ghost"
                      size="icon"
                      onClick={() => removeFixedValue(key)}
                      className="h-6 w-6"
                    >
                      <Trash2 className="h-3 w-3 text-destructive" />
                    </Button>
                  </div>
                ))}
                {Object.keys(command.fixed_values || {}).length === 0 && (
                  <div className="text-xs text-muted-foreground italic text-center py-2">
                    {t('devices:commandEditor.noFixedValues')}
                  </div>
                )}
              </div>
            )}
          </div>
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
  // Hooks must be called before any early returns
  // Fetch transforms that apply to this device type
  const [virtualMetrics, setVirtualMetrics] = React.useState<Array<{
    name: string
    display_name: string
    data_type: string
    unit?: string
    transform_id: string
    transform_name: string
  }>>([])
  const [loadingVirtual, setLoadingVirtual] = React.useState(false)

  React.useEffect(() => {
    if (open && deviceType?.device_type) {
      setLoadingVirtual(true)
      api.listTransforms()
        .then(data => {
          const transforms: any[] = data.transforms || []

          // Filter transforms that apply to this device type
          const matching = transforms
            .filter((t: any) => {
              if (!t.enabled) return false

              // Handle scope - might be string or already parsed object
              let scope = t.scope
              if (typeof scope === 'string') {
                try {
                  scope = JSON.parse(scope)
                } catch {
                  // If parse fails, it might be just "global"
                }
              }

              // Check if scope matches this device type
              if (typeof scope === 'object' && scope !== null && 'device_type' in scope) {
                return scope.device_type === deviceType.device_type
              }
              return false
            })
            .map((t: any) => {
              // Parse js_code to extract output metrics
              const prefix = t.output_prefix || 'transform'
              const outputs = parseTransformOutputs(t.js_code, prefix)
              return outputs.map((out: any) => ({
                ...out,
                transform_id: t.id,
                transform_name: t.name,
              }))
            })
            .flat()

          setVirtualMetrics(matching)
        })
        .catch(() => setVirtualMetrics([]))
        .finally(() => setLoadingVirtual(false))
    }
  }, [open, deviceType?.device_type])

  // Early return after all hooks
  if (!deviceType) return null

  const isRawMode = deviceType.mode === 'simple'

  // Parse transform JS code to extract output metrics
  const parseTransformOutputs = (jsCode: string, prefix: string) => {
    const outputs: Array<{ name: string; display_name: string; data_type: string; unit?: string }> = []

    // Try to infer data type from the value/expression
    const inferType = (_propName: string, value: string): string => {
      const lowerVal = value.toLowerCase()

      // Boolean checks
      if (lowerVal === 'true' || lowerVal === 'false' || lowerVal.includes('!') || lowerVal.includes('===') || lowerVal.includes('&&') || lowerVal.includes('||')) {
        return 'boolean'
      }

      // Number checks - arithmetic operations or numeric literals
      if (/^[\d\s+\-*/().]+$/.test(value) || lowerVal.includes('parsefloat') || lowerVal.includes('parseint') || lowerVal.includes('math.') || lowerVal.includes('number(')) {
        return 'float'
      }

      // String checks
      if (value.startsWith('"') || value.startsWith("'") || value.includes('toString()') || value.includes('string(')) {
        return 'string'
      }

      // Array checks
      if (value.includes('[') || value.includes('Array(') || value.includes('push(')) {
        return 'array'
      }

      // Object checks
      if (value.includes('{') || value.includes('Object.create')) {
        return 'object'
      }

      // Check if accessing a metric from input - might inherit its type
      if (lowerVal.includes('input.') || lowerVal.includes('values.')) {
        // Default to float for telemetry data
        return 'float'
      }

      return 'unknown'
    }

    try {
      // Look for return statement patterns
      const returnMatch = jsCode.match(/return\s+({[\s\S]*?});?\s*(?:\/\/.*)?$/m)
      if (returnMatch) {
        const returnObj = returnMatch[1]
        // Extract property names and their values from return object
        const propMatches = returnObj.matchAll(/(\w+)\s*:\s*([^,}]+)/g)
        for (const match of propMatches) {
          const propName = match[1]
          const propValue = match[2]?.trim()
          if (propName !== 'input' && propName !== '_raw') {
            const inferredType = inferType(propName, propValue)
            outputs.push({
              name: `${prefix}.${propName}`,
              display_name: propName,
              data_type: inferredType,
            })
          }
        }
      }

      // If no return object found, look for single value return
      if (outputs.length === 0) {
        const singleMatch = jsCode.match(/return\s+([^;{}]+);/)
        if (singleMatch) {
          const value = singleMatch[1]?.trim()
          outputs.push({
            name: `${prefix}.value`,
            display_name: 'Value',
            data_type: inferType('value', value),
          })
        }
      }
    } catch {
      // Fallback to generic output
    }

    return outputs.length > 0 ? outputs : [{
      name: `${prefix}.*`,
      display_name: 'Virtual Metric',
      data_type: 'float',
    }]
  }

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-4xl sm:h-[85vh] sm:max-h-[85vh] flex flex-col overflow-hidden">
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2 flex-wrap">
            {deviceType.name}
            <Badge variant="outline" className="text-xs font-normal">
              {deviceType.device_type}
            </Badge>
            {isRawMode && (
              <Badge variant="secondary" className="text-xs">
                <Zap className="h-3 w-3 mr-1" />
                Raw Data Mode
              </Badge>
            )}
          </DialogTitle>
          <DialogDescription className="flex items-center gap-2 mt-1">
            Device type details and capabilities
          </DialogDescription>
        </DialogHeader>

        <DialogContentBody className="flex-1 overflow-y-auto">
          <div className="space-y-6">
            {/* Summary Cards */}
            <div className="grid grid-cols-4 gap-4">
              <Card className="p-4">
                <div className="flex items-center gap-3">
                  <div className="p-2 rounded-lg bg-green-100 dark:bg-green-900/30">
                    <ArrowDown className="h-5 w-5 text-green-600 dark:text-green-400" />
                  </div>
                  <div>
                    <div className="text-2xl font-bold">{deviceType.metrics?.length || 0}</div>
                    <div className="text-xs text-muted-foreground">Native Metrics</div>
                  </div>
                </div>
              </Card>
              <Card className="p-4">
                <div className="flex items-center gap-3">
                  <div className="p-2 rounded-lg bg-purple-100 dark:bg-purple-900/30">
                    <Sparkles className="h-5 w-5 text-purple-600 dark:text-purple-400" />
                  </div>
                  <div>
                    <div className="text-2xl font-bold">{virtualMetrics.length || 0}</div>
                    <div className="text-xs text-muted-foreground">Virtual Metrics</div>
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
                  <div className="p-2 rounded-lg bg-orange-100 dark:bg-orange-900/30">
                    <Settings className="h-5 w-5 text-orange-600 dark:text-orange-400" />
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
                  Native Metrics ({deviceType.metrics?.length || 0})
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
                          <Badge variant="outline" className="text-xs">{formatDataType(metric.data_type)}</Badge>
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

            {/* Virtual Metrics (from Transforms) */}
            {virtualMetrics.length > 0 && (
              <Card className="p-4">
                <div className="flex items-center justify-between mb-4">
                  <h4 className="font-medium flex items-center gap-2">
                    <Sparkles className="h-4 w-4 text-purple-500" />
                    Virtual Metrics ({virtualMetrics.length})
                  </h4>
                  <Badge variant="secondary" className="text-xs">
                    From Transforms
                  </Badge>
                </div>

                {loadingVirtual ? (
                  <div className="text-center py-4 text-muted-foreground text-sm">
                    Loading virtual metrics...
                  </div>
                ) : (
                  <div className="space-y-2">
                    {virtualMetrics.map((metric, i) => (
                      <div key={i} className="p-3 bg-purple-500/10 border border-purple-500/20 rounded-lg">
                        <div className="flex items-center justify-between">
                          <div className="flex items-center gap-2">
                            <Sparkles className="h-3 w-3 text-purple-500" />
                            <code className="text-sm text-purple-700 dark:text-purple-300">{metric.name}</code>
                            <span className="text-muted-foreground">•</span>
                            <span className="text-sm">{metric.display_name}</span>
                          </div>
                          <div className="flex items-center gap-2">
                            <Badge variant="outline" className="text-xs text-purple-500">{formatDataType(metric.data_type)}</Badge>
                            <span className="text-xs text-muted-foreground">via {metric.transform_name}</span>
                          </div>
                        </div>
                      </div>
                    ))}
                  </div>
                )}
              </Card>
            )}

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
                          {cmd.parameters?.length || 0} params
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
        </DialogContentBody>

        <DialogFooter>
          <Button variant="outline" onClick={() => onOpenChange(false)}>
            Close
          </Button>
        </DialogFooter>
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
}

// Reuse AddDeviceTypeDialog with editDeviceType prop
export function EditDeviceTypeDialog({ open, onOpenChange, deviceType, onEdit, editing }: EditDeviceTypeDialogProps) {
  // Default no-op validator for edit mode
  const handleValidate = async (): Promise<ValidationResult> => ({
    valid: true,
    message: "Ready to save"
  })

  return (
    <AddDeviceTypeDialog
      open={open}
      onOpenChange={onOpenChange}
      onAdd={onEdit}
      onValidate={handleValidate}
      adding={editing}
      validating={false}
      editDeviceType={deviceType}
    />
  )
}

// ============================================================================
// GITHUB IMPORT DIALOG - Simplified for importing from official repository
// ============================================================================

interface CloudImportDialogProps {
  open: boolean
  onOpenChange: (open: boolean) => void
  onImportComplete?: () => void
}

interface CloudDeviceType {
  device_type: string
  name: string
  description: string
  categories: string[]
  url?: string
  selected?: boolean
}

interface CloudImportResponse {
  device_types: CloudDeviceType[]
  total: number
}

export function CloudImportDialog({ open, onOpenChange, onImportComplete }: CloudImportDialogProps) {
  const { t } = useTranslation(['common', 'devices'])
  const { toast } = useToast()

  // UI state
  const [loading, setLoading] = useState(false)
  const [importing, setImporting] = useState(false)
  const [deviceTypes, setDeviceTypes] = useState<CloudDeviceType[]>([])
  const [selectedIds, setSelectedIds] = useState<Set<string>>(new Set())

  // Load device types from cloud repository
  useEffect(() => {
    if (open) {
      loadDeviceTypes()
    }
  }, [open])

  const loadDeviceTypes = async () => {
    setLoading(true)
    try {
      const res = await fetchAPI<CloudImportResponse & { error?: string; message?: string }>("/device-types/cloud/list")
      setDeviceTypes(res.device_types || [])

      // Show warning if there was an error but still got some results
      if (res.error && res.device_types?.length === 0) {
        toast({
          title: res.message || t('devices:cloud.loadFailed', "加载失败"),
          description: res.error === 'network_error'
            ? t('devices:cloud.networkError', "无法连接到 GitHub，请检查网络连接")
            : t('devices:cloud.loadFailedDesc', "无法加载云端设备类型"),
          variant: "destructive"
        })
      }
    } catch (e) {
      console.error("Failed to load device types:", e)
      toast({
        title: t('devices:cloud.loadFailed', "加载失败"),
        description: t('devices:cloud.loadFailedDesc', "无法加载云端设备类型"),
        variant: "destructive"
      })
    } finally {
      setLoading(false)
    }
  }

  const toggleSelection = (id: string) => {
    const newSelected = new Set(selectedIds)
    if (newSelected.has(id)) {
      newSelected.delete(id)
    } else {
      newSelected.add(id)
    }
    setSelectedIds(newSelected)
  }

  const selectAll = () => {
    setSelectedIds(new Set(deviceTypes.map(d => d.device_type)))
  }

  const clearSelection = () => {
    setSelectedIds(new Set())
  }

  const handleImport = async () => {
    if (selectedIds.size === 0) {
      toast({
        title: t('devices:cloud.noSelection', "未选择"),
        description: t('devices:cloud.noSelectionDesc', "请至少选择一个设备类型"),
        variant: "destructive"
      })
      return
    }

    setImporting(true)
    try {
      const response = await fetchAPI<{ imported: number; skipped: number; failed: number; failures?: Array<{ device_type: string; reason: string }> }>("/device-types/cloud/import", {
        method: "POST",
        body: JSON.stringify({
          device_types: Array.from(selectedIds)
        })
      })

      // Build detailed description
      let description = `导入 ${response.imported} 个`
      if (response.skipped > 0) {
        description += `，跳过 ${response.skipped} 个（已存在）`
      }
      if (response.failed > 0) {
        description += `，失败 ${response.failed} 个`
      }

      const variant = response.failed > 0 ? "destructive" : "default"

      toast({
        title: response.imported > 0
          ? t('devices:cloud.importSuccess', "导入成功")
          : t('devices:cloud.importPartial', "导入完成"),
        description,
        variant,
      })

      // Log failures for debugging
      if (response.failures && response.failures.length > 0) {
        console.warn("Failed device type imports:", response.failures)
      }

      onImportComplete?.()
      setTimeout(() => {
        onOpenChange(false)
        setSelectedIds(new Set())
      }, 1000)
    } catch (e: any) {
      toast({
        title: t('devices:cloud.importFailed', "导入失败"),
        description: e?.message || t('devices:cloud.importFailedDesc', "导入设备类型失败"),
        variant: "destructive"
      })
    } finally {
      setImporting(false)
    }
  }

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-3xl sm:max-h-[80vh] overflow-hidden flex flex-col">
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2">
            <Download className="h-5 w-5" />
            {t('devices:cloud.importTitle', "从云端导入设备类型")}
          </DialogTitle>
          <DialogDescription>
            {t('devices:cloud.importDescription', "选择需要的设备类型快速添加到系统")}
          </DialogDescription>
        </DialogHeader>

        <DialogContentBody className="flex-1 overflow-y-auto">
          {loading ? (
            <div className="flex items-center justify-center py-12">
              <Loader2 className="h-6 w-6 animate-spin text-muted-foreground" />
              <span className="ml-2 text-muted-foreground">
                {t('devices:cloud.loading', "加载中...")}
              </span>
            </div>
          ) : deviceTypes.length === 0 ? (
            <div className="text-center py-12 text-muted-foreground">
              {t('devices:cloud.noDeviceTypes', "暂无可用设备类型")}
            </div>
          ) : (
            <div className="space-y-4 py-4">
              {/* Select All / Clear */}
              <div className="flex items-center justify-between text-sm">
                <span className="text-muted-foreground">
                  {t('devices:cloud.selectedCount', `已选择 {{count}} 个`, { count: selectedIds.size })}
                </span>
                <div className="flex gap-2">
                  <Button variant="ghost" size="sm" onClick={selectAll}>
                    {t('devices:cloud.selectAll', "全选")}
                  </Button>
                  <Button variant="ghost" size="sm" onClick={clearSelection}>
                    {t('devices:cloud.clear', "清空")}
                  </Button>
                </div>
              </div>

              {/* Device Type List */}
              <div className="grid gap-2">
                {deviceTypes.map((dt) => (
                  <div
                    key={dt.device_type}
                    className={cn(
                      "flex items-start gap-3 p-3 rounded-lg border cursor-pointer transition-colors",
                      selectedIds.has(dt.device_type)
                        ? "bg-primary/10 border-primary"
                        : "hover:bg-accent"
                    )}
                    onClick={() => toggleSelection(dt.device_type)}
                  >
                    <div className="mt-0.5">
                      <div className={cn(
                        "w-4 h-4 rounded border flex items-center justify-center",
                        selectedIds.has(dt.device_type)
                          ? "bg-primary border-primary"
                          : "border-muted-foreground"
                      )}>
                        {selectedIds.has(dt.device_type) && (
                          <Check className="h-3 w-3 text-primary-foreground" />
                        )}
                      </div>
                    </div>
                    <div className="flex-1 min-w-0">
                      <div className="font-medium">{dt.name}</div>
                      <div className="text-xs text-muted-foreground">{dt.device_type}</div>
                      {dt.description && (
                        <div className="text-sm text-muted-foreground mt-1 line-clamp-2">
                          {dt.description}
                        </div>
                      )}
                      {dt.categories.length > 0 && (
                        <div className="flex gap-1 mt-2">
                          {dt.categories.map((cat) => (
                            <Badge key={cat} variant="outline" className="text-xs">
                              {cat}
                            </Badge>
                          ))}
                        </div>
                      )}
                    </div>
                  </div>
                ))}
              </div>
            </div>
          )}
        </DialogContentBody>

        <DialogFooter>
          <Button
            variant="outline"
            onClick={() => onOpenChange(false)}
            disabled={importing}
          >
            {t('common:cancel')}
          </Button>
          <Button
            onClick={handleImport}
            disabled={importing || selectedIds.size === 0}
          >
            {importing ? (
              <>
                <Loader2 className="h-4 w-4 mr-2 animate-spin" />
                {t('devices:cloud.importing', "导入中...")}
              </>
            ) : (
              <>
                <Download className="h-4 w-4 mr-2" />
                {t('devices:cloud.importButton', "导入 ({{count}})", { count: selectedIds.size })}
              </>
            )}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
