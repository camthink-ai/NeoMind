import { useState, useCallback } from 'react'
import { useTranslation } from 'react-i18next'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Textarea } from '@/components/ui/textarea'
import { Switch } from '@/components/ui/switch'
import { Badge } from '@/components/ui/badge'
import { Tabs, TabsList, TabsTrigger, TabsContent } from '@/components/ui/tabs'
import {
  Plus,
  Trash2,
  GripVertical,
  Play,
  Pause,
  Clock,
  AlertTriangle,
  Database,
  Globe,
  Code,
  Split,
  GitBranch,
  Zap,
  Bell,
  Image as ImageIcon,
  FileText,
  Braces,
  ArrowUp,
  ArrowDown,
  Edit3,
  Sparkles,
  Info,
  Lightbulb,
  Settings as SettingsIcon,
} from 'lucide-react'
import type {
  Workflow,
  WorkflowStep,
  WorkflowStepType,
  WorkflowTrigger,
  WorkflowTriggerType,
} from '@/types'
import { StepConfigDialog } from './workflow/StepConfigDialog'
import { TriggerConfigDialog } from './workflow/TriggerConfigDialog'
import {
  FullScreenBuilder,
  BuilderSection,
  FormGrid,
  TipCard,
} from './FullScreenBuilder'

interface WorkflowBuilderProps {
  open: boolean
  onClose: () => void
  workflow?: Workflow
  onSave: (workflow: Partial<Workflow>) => Promise<void>
  resources?: {
    devices: Array<{ id: string; name: string }>
    metrics: string[]
    alertChannels: Array<{ id: string; name: string }>
  }
}

interface WorkflowFormData {
  id: string
  name: string
  description: string
  enabled: boolean
  steps: WorkflowStep[]
  triggers: WorkflowTrigger[]
  variables: Record<string, unknown>
  timeout_seconds: number
}

// Step type definitions with icons and labels
const STEP_TYPES: Array<{
  type: WorkflowStepType
  icon: React.ReactNode
  label: string
  description: string
  category: 'device' | 'logic' | 'action' | 'advanced'
}> = [
  {
    type: 'send_command',
    icon: <Zap className="h-4 w-4" />,
    label: 'automation:steps.sendCommand',
    description: 'automation:steps.sendCommandDesc',
    category: 'device',
  },
  {
    type: 'device_query',
    icon: <Database className="h-4 w-4" />,
    label: 'automation:steps.deviceQuery',
    description: 'automation:steps.deviceQueryDesc',
    category: 'device',
  },
  {
    type: 'wait_for_device_state',
    icon: <Clock className="h-4 w-4" />,
    label: 'automation:steps.waitForDeviceState',
    description: 'automation:steps.waitForDeviceStateDesc',
    category: 'device',
  },
  {
    type: 'condition',
    icon: <GitBranch className="h-4 w-4" />,
    label: 'automation:steps.condition',
    description: 'automation:steps.conditionDesc',
    category: 'logic',
  },
  {
    type: 'delay',
    icon: <Pause className="h-4 w-4" />,
    label: 'automation:steps.delay',
    description: 'automation:steps.delayDesc',
    category: 'logic',
  },
  {
    type: 'parallel',
    icon: <Split className="h-4 w-4" />,
    label: 'automation:steps.parallel',
    description: 'automation:steps.parallelDesc',
    category: 'logic',
  },
  {
    type: 'send_alert',
    icon: <Bell className="h-4 w-4" />,
    label: 'automation:steps.sendAlert',
    description: 'automation:steps.sendAlertDesc',
    category: 'action',
  },
  {
    type: 'log',
    icon: <FileText className="h-4 w-4" />,
    label: 'automation:steps.log',
    description: 'automation:steps.logDesc',
    category: 'action',
  },
  {
    type: 'http_request',
    icon: <Globe className="h-4 w-4" />,
    label: 'automation:steps.httpRequest',
    description: 'automation:steps.httpRequestDesc',
    category: 'action',
  },
  {
    type: 'data_query',
    icon: <Database className="h-4 w-4" />,
    label: 'automation:steps.dataQuery',
    description: 'automation:steps.dataQueryDesc',
    category: 'advanced',
  },
  {
    type: 'execute_wasm',
    icon: <Code className="h-4 w-4" />,
    label: 'automation:steps.executeWasm',
    description: 'automation:steps.executeWasmDesc',
    category: 'advanced',
  },
  {
    type: 'image_process',
    icon: <ImageIcon className="h-4 w-4" />,
    label: 'automation:steps.imageProcess',
    description: 'automation:steps.imageProcessDesc',
    category: 'advanced',
  },
]

// Trigger type definitions
const TRIGGER_TYPES: Array<{
  type: WorkflowTriggerType
  icon: React.ReactNode
  label: string
  description: string
}> = [
  {
    type: 'manual',
    icon: <Play className="h-4 w-4" />,
    label: 'automation:triggers.manual',
    description: 'automation:triggers.manualDesc',
  },
  {
    type: 'cron',
    icon: <Clock className="h-4 w-4" />,
    label: 'automation:triggers.cron',
    description: 'automation:triggers.cronDesc',
  },
  {
    type: 'event',
    icon: <AlertTriangle className="h-4 w-4" />,
    label: 'automation:triggers.event',
    description: 'automation:triggers.eventDesc',
  },
  {
    type: 'device',
    icon: <Zap className="h-4 w-4" />,
    label: 'automation:triggers.device',
    description: 'automation:triggers.deviceDesc',
  },
]

export function WorkflowBuilder({
  open,
  onClose,
  workflow,
  onSave,
  resources = { devices: [], metrics: [], alertChannels: [] },
}: WorkflowBuilderProps) {
  const { t } = useTranslation(['automation', 'common'])

  const [formData, setFormData] = useState<WorkflowFormData>({
    id: workflow?.id || `workflow-${Date.now()}`,
    name: workflow?.name || '',
    description: workflow?.description || '',
    enabled: workflow?.enabled ?? true,
    steps: (workflow?.steps as WorkflowStep[]) || [],
    triggers: (workflow?.triggers as WorkflowTrigger[]) || [{ type: 'manual', id: 'trigger-manual' }],
    variables: (workflow?.variables as Record<string, unknown>) || {},
    timeout_seconds: 300,
  })
  const [saving, setSaving] = useState(false)
  const [activeTab, setActiveTab] = useState<'steps' | 'triggers' | 'variables' | 'settings'>('steps')

  // Reset form when workflow changes
  useState(() => {
    if (open && workflow) {
      setFormData({
        id: workflow.id,
        name: workflow.name || '',
        description: workflow.description || '',
        enabled: workflow.enabled ?? true,
        steps: (workflow.steps as WorkflowStep[]) || [],
        triggers: (workflow.triggers as WorkflowTrigger[]) || [{ type: 'manual', id: 'trigger-manual' }],
        variables: (workflow.variables as Record<string, unknown>) || {},
        timeout_seconds: 300,
      })
    } else if (open && !workflow) {
      setFormData({
        id: `workflow-${Date.now()}`,
        name: '',
        description: '',
        enabled: true,
        steps: [],
        triggers: [{ type: 'manual', id: 'trigger-manual' }],
        variables: {},
        timeout_seconds: 300,
      })
    }
  })

  // Step configuration dialog state
  const [stepDialogOpen, setStepDialogOpen] = useState(false)
  const [editingStep, setEditingStep] = useState<WorkflowStep | null>(null)
  const [stepIndex, setStepIndex] = useState<number | null>(null)

  // Trigger configuration dialog state
  const [triggerDialogOpen, setTriggerDialogOpen] = useState(false)
  const [editingTrigger, setEditingTrigger] = useState<WorkflowTrigger | null>(null)
  const [triggerIndex, setTriggerIndex] = useState<number | null>(null)

  // New variable state
  const [newVarName, setNewVarName] = useState('')
  const [newVarValue, setNewVarValue] = useState('')

  const updateFormData = useCallback((updates: Partial<WorkflowFormData>) => {
    setFormData((prev) => ({ ...prev, ...updates }))
  }, [])

  // Generate a unique step ID
  const generateStepId = useCallback(() => {
    return `step-${Date.now()}-${Math.random().toString(36).substr(2, 9)}`
  }, [])

  // Add a new step
  const handleAddStep = useCallback(
    (type: WorkflowStepType) => {
      const newStep: WorkflowStep = {
        id: generateStepId(),
        type,
      } as WorkflowStep

      // Set default values based on step type
      switch (type) {
        case 'delay':
          ;(newStep as any).duration_seconds = 5
          break
        case 'send_alert':
          ;(newStep as any).severity = 'info'
          ;(newStep as any).title = ''
          ;(newStep as any).message = ''
          break
        case 'log':
          ;(newStep as any).message = ''
          ;(newStep as any).level = 'info'
          break
        case 'condition':
          ;(newStep as any).condition = ''
          ;(newStep as any).then_steps = []
          ;(newStep as any).else_steps = []
          break
        case 'parallel':
          ;(newStep as any).steps = []
          break
        case 'http_request':
          ;(newStep as any).method = 'GET'
          ;(newStep as any).url = ''
          break
        case 'send_command':
          ;(newStep as any).device_id = ''
          ;(newStep as any).command = ''
          ;(newStep as any).parameters = {}
          break
        case 'device_query':
          ;(newStep as any).device_id = ''
          ;(newStep as any).metric = ''
          break
        case 'wait_for_device_state':
          ;(newStep as any).device_id = ''
          ;(newStep as any).metric = ''
          ;(newStep as any).expected_value = 0
          ;(newStep as any).timeout_seconds = 60
          ;(newStep as any).poll_interval_seconds = 5
          break
      }

      setEditingStep(newStep)
      setStepIndex(null)
      setStepDialogOpen(true)
    },
    [generateStepId]
  )

  // Edit an existing step
  const handleEditStep = useCallback((index: number) => {
    setEditingStep(formData.steps[index])
    setStepIndex(index)
    setStepDialogOpen(true)
  }, [formData.steps])

  // Delete a step
  const handleDeleteStep = useCallback((index: number) => {
    setFormData((prev) => ({
      ...prev,
      steps: prev.steps.filter((_, i) => i !== index),
    }))
  }, [])

  // Move a step up
  const handleMoveStepUp = useCallback((index: number) => {
    if (index === 0) return
    setFormData((prev) => {
      const steps = [...prev.steps]
      ;[steps[index - 1], steps[index]] = [steps[index], steps[index - 1]]
      return { ...prev, steps }
    })
  }, [])

  // Move a step down
  const handleMoveStepDown = useCallback((index: number) => {
    if (index >= formData.steps.length - 1) return
    setFormData((prev) => {
      const steps = [...prev.steps]
      ;[steps[index], steps[index + 1]] = [steps[index + 1], steps[index]]
      return { ...prev, steps }
    })
  }, [formData.steps.length])

  // Save step from dialog
  const handleSaveStep = useCallback((step: WorkflowStep) => {
    setFormData((prev) => {
      const steps = [...prev.steps]
      if (stepIndex !== null) {
        steps[stepIndex] = step
      } else {
        steps.push(step)
      }
      return { ...prev, steps }
    })
    setStepDialogOpen(false)
    setEditingStep(null)
    setStepIndex(null)
  }, [stepIndex])

  // Add a new trigger
  const handleAddTrigger = useCallback((type: WorkflowTriggerType) => {
    const newTrigger: WorkflowTrigger = {
      id: `trigger-${Date.now()}`,
      type,
    } as WorkflowTrigger

    // Set default values based on trigger type
    switch (type) {
      case 'cron':
        ;(newTrigger as any).expression = '0 * * * *'
        break
      case 'event':
        ;(newTrigger as any).event_type = ''
        break
      case 'device':
        ;(newTrigger as any).device_id = ''
        ;(newTrigger as any).metric = ''
        ;(newTrigger as any).condition = '>'
        break
    }

    setEditingTrigger(newTrigger)
    setTriggerIndex(null)
    setTriggerDialogOpen(true)
  }, [])

  // Edit an existing trigger
  const handleEditTrigger = useCallback((index: number) => {
    setEditingTrigger(formData.triggers[index])
    setTriggerIndex(index)
    setTriggerDialogOpen(true)
  }, [formData.triggers])

  // Delete a trigger
  const handleDeleteTrigger = useCallback((index: number) => {
    setFormData((prev) => ({
      ...prev,
      triggers: prev.triggers.filter((_, i) => i !== index),
    }))
  }, [])

  // Save trigger from dialog
  const handleSaveTrigger = useCallback((trigger: WorkflowTrigger) => {
    setFormData((prev) => {
      const triggers = [...prev.triggers]
      if (triggerIndex !== null) {
        triggers[triggerIndex] = trigger
      } else {
        triggers.push(trigger)
      }
      return { ...prev, triggers }
    })
    setTriggerDialogOpen(false)
    setEditingTrigger(null)
    setTriggerIndex(null)
  }, [triggerIndex])

  // Add a variable
  const handleAddVariable = useCallback(() => {
    if (!newVarName.trim()) return
    setFormData((prev) => ({
      ...prev,
      variables: {
        ...prev.variables,
        [newVarName]: newVarValue,
      },
    }))
    setNewVarName('')
    setNewVarValue('')
  }, [newVarName, newVarValue])

  // Delete a variable
  const handleDeleteVariable = useCallback((key: string) => {
    setFormData((prev) => {
      const vars = { ...prev.variables }
      delete vars[key]
      return { ...prev, variables: vars }
    })
  }, [])

  // Get step icon
  const getStepIcon = (type: WorkflowStepType) => {
    return STEP_TYPES.find((s) => s.type === type)?.icon || <Braces className="h-4 w-4" />
  }

  // Get step label
  const getStepLabel = (step: WorkflowStep) => {
    const stepType = STEP_TYPES.find((s) => s.type === step.type)
    if (!stepType) return t(`automation:steps.${step.type}`)

    // Add additional info based on step type
    switch (step.type) {
      case 'send_command':
        return `${t(stepType.label)} (${(step as any).command || 'N/A'})`
      case 'delay':
        return `${t(stepType.label)} (${(step as any).duration_seconds}s)`
      case 'send_alert':
        return `${t(stepType.label)} (${(step as any).title || 'N/A'})`
      case 'log':
        return `${t(stepType.label)}: ${(step as any).message?.substring(0, 30) || ''}...`
      case 'condition':
        return `${t(stepType.label)} (${(step as any).condition || 'N/A'})`
      case 'http_request':
        return `${t(stepType.label)} (${(step as any).method})`
      case 'device_query':
        return `${t(stepType.label)} (${(step as any).metric || 'N/A'})`
      default:
        return t(stepType.label)
    }
  }

  // Get trigger icon
  const getTriggerIcon = (type: WorkflowTriggerType) => {
    return TRIGGER_TYPES.find((t) => t.type === type)?.icon || <Clock className="h-4 w-4" />
  }

  // Get trigger label
  const getTriggerLabel = (trigger: WorkflowTrigger) => {
    switch (trigger.type) {
      case 'manual':
        return t('automation:triggers.manual')
      case 'cron':
        return `${t('automation:triggers.cron')}: ${(trigger as any).expression}`
      case 'event':
        return `${t('automation:triggers.event')}: ${(trigger as any).event_type || 'N/A'}`
      case 'device':
        return `${t('automation:triggers.device')}: ${(trigger as any).metric || 'N/A'}`
    }
  }

  // Validate and save
  const isValid = Boolean(formData.name.trim() && formData.steps.length > 0)
  const getValidationMessage = () => {
    if (!formData.name.trim()) return t('automation:validation.nameRequired', { defaultValue: '请输入名称' })
    if (formData.steps.length === 0) return t('automation:validation.stepsRequired', { defaultValue: '请至少添加一个步骤' })
    return ''
  }

  const handleSave = useCallback(async () => {
    if (!isValid) return

    setSaving(true)
    try {
      await onSave({
        id: formData.id,
        name: formData.name,
        description: formData.description,
        enabled: formData.enabled,
        steps: formData.steps,
        triggers: formData.triggers,
        variables: formData.variables,
        timeout_seconds: formData.timeout_seconds,
      })
    } finally {
      setSaving(false)
    }
  }, [formData, isValid, onSave])

  // Side panel content
  const sidePanelContent = (
    <div className="space-y-4">
      <TipCard
        title={t('automation:tips.workflowTitle', { defaultValue: '关于工作流' })}
        variant="info"
      >
        {t('automation:tips.workflowDesc', {
          defaultValue: '工作流是复杂的多步骤自动化，支持条件分支、并行执行、延迟等待等功能。',
        })}
      </TipCard>

      {activeTab === 'steps' && (
        <TipCard
          title={t('automation:tips.stepsTitle', { defaultValue: '步骤说明' })}
          variant="info"
        >
          {t('automation:tips.stepsDesc', {
            defaultValue: '工作流步骤按顺序执行。条件步骤支持分支，并行步骤可同时执行多个任务。',
          })}
        </TipCard>
      )}

      {activeTab === 'triggers' && (
        <TipCard
          title={t('automation:tips.triggersTitle', { defaultValue: '触发器说明' })}
          variant="info"
        >
          {t('automation:tips.triggersDesc', {
            defaultValue: '定义何时启动工作流。可以手动触发、定时执行、响应事件或设备状态变化。',
          })}
        </TipCard>
      )}

      {activeTab === 'variables' && (
        <TipCard
          title={t('automation:tips.variablesTitle', { defaultValue: '变量说明' })}
          variant="info"
        >
          {t('automation:tips.variablesDesc', {
            defaultValue: '在工作流中使用变量。可以在步骤中引用这些变量，格式为 ${varName}。',
          })}
        </TipCard>
      )}
    </div>
  )

  return (
    <FullScreenBuilder
      open={open}
      onClose={onClose}
      title={workflow
        ? t('automation:editWorkflow', { defaultValue: '编辑工作流' })
        : t('automation:createWorkflow', { defaultValue: '创建工作流' })
      }
      description={t('automation:workflowBuilderDesc', {
        defaultValue: '定义复杂的多步骤自动化流程，支持条件分支、并行执行等高级功能',
      })}
      icon={<Sparkles className="h-5 w-5 text-blue-500" />}
      headerActions={
        <Badge variant={formData.enabled ? 'default' : 'secondary'} className="text-xs">
          {formData.enabled ? t('common:enabled', { defaultValue: '启用' }) : t('common:disabled', { defaultValue: '禁用' })}
        </Badge>
      }
      sidePanel={{ content: sidePanelContent, title: t('automation:tips', { defaultValue: '提示' }) }}
      isValid={isValid}
      isDirty={true}
      isSaving={saving}
      saveLabel={t('common:save', { defaultValue: '保存' })}
      onSave={handleSave}
      validationMessage={getValidationMessage()}
    >
      <div className="space-y-6">
        {/* Basic Info Section */}
        <BuilderSection
          title={t('automation:basicInfo', { defaultValue: '基本信息' })}
          icon={<Info className="h-4 w-4 text-muted-foreground" />}
        >
          <FormGrid columns={2}>
            <div className="space-y-2">
              <Label htmlFor="workflow-name">{t('automation:workflowName', { defaultValue: '工作流名称' })} *</Label>
              <Input
                id="workflow-name"
                value={formData.name}
                onChange={(e) => updateFormData({ name: e.target.value })}
                placeholder={t('automation:workflowNamePlaceholder', { defaultValue: '例如：多步骤设备控制流程' })}
              />
            </div>
            <div className="flex items-center gap-2">
              <Switch
                id="workflow-enabled"
                checked={formData.enabled}
                onCheckedChange={(enabled) => updateFormData({ enabled })}
              />
              <Label htmlFor="workflow-enabled" className="text-sm cursor-pointer">
                {t('automation:enableWorkflow', { defaultValue: '启用工作流' })}
              </Label>
            </div>
          </FormGrid>
          <div className="space-y-2">
            <Label htmlFor="workflow-description">{t('common:description', { defaultValue: '描述' })}</Label>
            <Textarea
              id="workflow-description"
              value={formData.description}
              onChange={(e) => updateFormData({ description: e.target.value })}
              placeholder={t('automation:workflowDescriptionPlaceholder', {
                defaultValue: '描述这个工作流的功能和用途',
              })}
              rows={2}
            />
          </div>
        </BuilderSection>

        {/* Main Tabs */}
        <Tabs value={activeTab} onValueChange={(v) => setActiveTab(v as any)} className="w-full">
          <TabsList className="grid w-full grid-cols-4">
            <TabsTrigger value="steps" className="gap-2">
              <Braces className="h-4 w-4" />
              <span>{t('automation:steps', { defaultValue: '步骤' })}</span>
              <Badge variant="secondary" className="ml-auto h-5 px-1.5 text-xs">
                {formData.steps.length}
              </Badge>
            </TabsTrigger>
            <TabsTrigger value="triggers" className="gap-2">
              <Clock className="h-4 w-4" />
              <span>{t('automation:triggers', { defaultValue: '触发器' })}</span>
              <Badge variant="secondary" className="ml-auto h-5 px-1.5 text-xs">
                {formData.triggers.length}
              </Badge>
            </TabsTrigger>
            <TabsTrigger value="variables" className="gap-2">
              <Database className="h-4 w-4" />
              <span>{t('automation:variables', { defaultValue: '变量' })}</span>
              <Badge variant="secondary" className="ml-auto h-5 px-1.5 text-xs">
                {Object.keys(formData.variables).length}
              </Badge>
            </TabsTrigger>
            <TabsTrigger value="settings" className="gap-2">
              <SettingsIcon className="h-4 w-4" />
              <span>{t('common:settings', { defaultValue: '设置' })}</span>
            </TabsTrigger>
          </TabsList>

          {/* Steps Tab */}
          <TabsContent value="steps" className="mt-6">
            <BuilderSection
              title={t('automation:workflowSteps', { defaultValue: '工作流步骤' })}
              description={t('automation:workflowStepsDesc', { defaultValue: '添加和管理工作流执行步骤' })}
              icon={<Braces className="h-4 w-4 text-blue-500" />}
            >
              {/* Step Type Selector */}
              <div className="mb-4">
                <Label className="mb-2 block">{t('automation:addStep', { defaultValue: '添加步骤' })}</Label>
                <div className="grid grid-cols-6 gap-2">
                  {STEP_TYPES.map((stepType) => (
                    <Button
                      key={stepType.type}
                      variant="outline"
                      size="sm"
                      onClick={() => handleAddStep(stepType.type)}
                      className="flex flex-col items-center gap-1 h-auto py-2"
                      title={t(stepType.description)}
                    >
                      {stepType.icon}
                      <span className="text-xs">{t(stepType.label)}</span>
                    </Button>
                  ))}
                </div>
              </div>

              {/* Steps List */}
              {formData.steps.length === 0 ? (
                <div className="text-center py-12 text-muted-foreground border-2 border-dashed rounded-lg">
                  <Braces className="h-12 w-12 mx-auto mb-4 opacity-50" />
                  <p className="font-medium">{t('automation:noSteps', { defaultValue: '暂无步骤' })}</p>
                  <p className="text-sm">{t('automation:noStepsDesc', { defaultValue: '点击上方按钮添加步骤' })}</p>
                </div>
              ) : (
                <div className="space-y-2">
                  {formData.steps.map((step, index) => (
                    <div
                      key={step.id}
                      className="flex items-center gap-2 p-3 bg-muted/30 rounded-lg border group hover:bg-muted/50 transition-colors"
                    >
                      <GripVertical className="h-5 w-5 text-muted-foreground cursor-move" />
                      <span className="text-xs text-muted-foreground w-6">{index + 1}</span>
                      <div className="flex items-center gap-2 px-2 py-1 bg-background rounded">
                        {getStepIcon(step.type)}
                      </div>
                      <div className="flex-1 min-w-0">
                        <div className="font-medium truncate">{getStepLabel(step)}</div>
                        <div className="text-xs text-muted-foreground truncate">
                          {t(`automation:steps.${step.type}Desc`)}
                        </div>
                      </div>
                      <div className="flex items-center gap-1">
                        <Button
                          variant="ghost"
                          size="icon"
                          className="h-7 w-7"
                          onClick={() => handleMoveStepUp(index)}
                          disabled={index === 0}
                        >
                          <ArrowUp className="h-3 w-3" />
                        </Button>
                        <Button
                          variant="ghost"
                          size="icon"
                          className="h-7 w-7"
                          onClick={() => handleMoveStepDown(index)}
                          disabled={index >= formData.steps.length - 1}
                        >
                          <ArrowDown className="h-3 w-3" />
                        </Button>
                        <Button
                          variant="ghost"
                          size="icon"
                          className="h-7 w-7"
                          onClick={() => handleEditStep(index)}
                        >
                          <Edit3 className="h-3 w-3" />
                        </Button>
                        <Button
                          variant="ghost"
                          size="icon"
                          className="h-7 w-7 text-destructive"
                          onClick={() => handleDeleteStep(index)}
                        >
                          <Trash2 className="h-3 w-3" />
                        </Button>
                      </div>
                    </div>
                  ))}
                </div>
              )}
            </BuilderSection>
          </TabsContent>

          {/* Triggers Tab */}
          <TabsContent value="triggers" className="mt-6">
            <BuilderSection
              title={t('automation:workflowTriggers', { defaultValue: '工作流触发器' })}
              description={t('automation:workflowTriggersDesc', { defaultValue: '定义何时启动此工作流' })}
              icon={<Clock className="h-4 w-4 text-yellow-500" />}
            >
              {/* Trigger Type Selector */}
              <div className="mb-4">
                <Label className="mb-2 block">{t('automation:addTrigger', { defaultValue: '添加触发器' })}</Label>
                <div className="flex flex-wrap gap-2">
                  {TRIGGER_TYPES.map((triggerType) => (
                    <Button
                      key={triggerType.type}
                      variant="outline"
                      size="sm"
                      onClick={() => handleAddTrigger(triggerType.type)}
                      className="flex items-center gap-2"
                    >
                      {triggerType.icon}
                      <span>{t(triggerType.label)}</span>
                    </Button>
                  ))}
                </div>
              </div>

              {/* Triggers List */}
              <div className="space-y-2">
                {formData.triggers.map((trigger, index) => (
                  <div
                    key={trigger.id}
                    className="flex items-center gap-2 p-3 bg-muted/30 rounded-lg border group hover:bg-muted/50 transition-colors"
                  >
                    <div className="flex items-center gap-2 px-2 py-1 bg-background rounded">
                      {getTriggerIcon(trigger.type)}
                    </div>
                    <div className="flex-1 font-medium">{getTriggerLabel(trigger)}</div>
                    <div className="flex items-center gap-1">
                      <Button
                        variant="ghost"
                        size="icon"
                        className="h-7 w-7"
                        onClick={() => handleEditTrigger(index)}
                      >
                        <Edit3 className="h-3 w-3" />
                      </Button>
                      {formData.triggers.length > 1 && (
                        <Button
                          variant="ghost"
                          size="icon"
                          className="h-7 w-7 text-destructive"
                          onClick={() => handleDeleteTrigger(index)}
                        >
                          <Trash2 className="h-3 w-3" />
                        </Button>
                      )}
                    </div>
                  </div>
                ))}
              </div>
            </BuilderSection>
          </TabsContent>

          {/* Variables Tab */}
          <TabsContent value="variables" className="mt-6">
            <BuilderSection
              title={t('automation:variables', { defaultValue: '工作流变量' })}
              description={t('automation:workflowVariablesDesc', { defaultValue: '定义可在工作流中使用的变量' })}
              icon={<Database className="h-4 w-4 text-green-500" />}
            >
              {/* Add Variable */}
              <div className="flex gap-2 mb-4">
                <Input
                  placeholder={t('automation:variableName', { defaultValue: '变量名' })}
                  value={newVarName}
                  onChange={(e) => setNewVarName(e.target.value)}
                  className="font-mono"
                />
                <Input
                  placeholder={t('automation:variableValue', { defaultValue: '变量值' })}
                  value={newVarValue}
                  onChange={(e) => setNewVarValue(e.target.value)}
                />
                <Button onClick={handleAddVariable} disabled={!newVarName.trim()}>
                  <Plus className="h-4 w-4" />
                </Button>
              </div>

              {/* Variables List */}
              <div className="space-y-2">
                {Object.entries(formData.variables).map(([key, value]) => (
                  <div
                    key={key}
                    className="flex items-center gap-2 p-3 bg-muted/30 rounded-lg border"
                  >
                    <div className="flex-1 font-mono text-sm">
                      <span className="text-blue-500">{key}</span>
                      <span className="text-muted-foreground mx-2">=</span>
                      <span>{JSON.stringify(value)}</span>
                    </div>
                    <Button
                      variant="ghost"
                      size="icon"
                      className="h-7 w-7 text-destructive"
                      onClick={() => handleDeleteVariable(key)}
                    >
                      <Trash2 className="h-3 w-3" />
                    </Button>
                  </div>
                ))}
                {Object.keys(formData.variables).length === 0 && (
                  <div className="text-center py-8 text-muted-foreground">
                    <Lightbulb className="h-8 w-8 mx-auto mb-2 opacity-50" />
                    <p className="text-sm">{t('automation:noVariables', { defaultValue: '暂无变量' })}</p>
                  </div>
                )}
              </div>
            </BuilderSection>
          </TabsContent>

          {/* Settings Tab */}
          <TabsContent value="settings" className="mt-6">
            <BuilderSection
              title={t('automation:workflowSettings', { defaultValue: '工作流设置' })}
              icon={<SettingsIcon className="h-4 w-4 text-muted-foreground" />}
            >
              <div className="space-y-4">
                <div>
                  <Label htmlFor="timeout">{t('automation:timeoutSeconds', { defaultValue: '超时时间（秒）' })}</Label>
                  <Input
                    id="timeout"
                    type="number"
                    value={formData.timeout_seconds}
                    onChange={(e) => updateFormData({ timeout_seconds: parseInt(e.target.value) || 300 })}
                    min={1}
                    max={3600}
                  />
                  <p className="text-xs text-muted-foreground mt-1">
                    {t('automation:timeoutDesc', { defaultValue: '工作流执行的最长时间，超时后将自动终止' })}
                  </p>
                </div>
              </div>
            </BuilderSection>
          </TabsContent>
        </Tabs>
      </div>

      {/* Step Configuration Dialog */}
      <StepConfigDialog
        open={stepDialogOpen}
        onOpenChange={setStepDialogOpen}
        step={editingStep}
        onSave={handleSaveStep}
        resources={resources}
      />

      {/* Trigger Configuration Dialog */}
      <TriggerConfigDialog
        open={triggerDialogOpen}
        onOpenChange={setTriggerDialogOpen}
        trigger={editingTrigger}
        onSave={handleSaveTrigger}
        resources={resources}
      />
    </FullScreenBuilder>
  )
}
