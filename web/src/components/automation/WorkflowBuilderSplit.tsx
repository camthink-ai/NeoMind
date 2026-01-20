import { useState, useCallback } from 'react'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Textarea } from '@/components/ui/textarea'
import { Switch } from '@/components/ui/switch'
import { Badge } from '@/components/ui/badge'
import {
  Trash2,
  Play,
  Clock,
  AlertTriangle,
  Zap,
  Bell,
  FileText,
  Code,
  Braces,
  ArrowUp,
  ArrowDown,
  Edit3,
  Sparkles,
  GripVertical,
} from 'lucide-react'
import type {
  Workflow,
  WorkflowStep,
  WorkflowStepType,
  WorkflowTrigger,
  WorkflowTriggerType,
} from '@/types'
import {
  SplitPaneBuilder,
  FormSection,
} from './SplitPaneBuilder'
import { cn } from '@/lib/utils'

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

// Step type definitions with icons
const STEP_TYPES: Array<{
  type: WorkflowStepType
  icon: React.ReactNode
  label: string
  category: 'device' | 'logic' | 'action' | 'advanced'
}> = [
  { type: 'send_command', icon: <Zap className="h-4 w-4" />, label: '执行命令', category: 'device' },
  { type: 'device_query', icon: <Code className="h-4 w-4" />, label: '设备查询', category: 'device' },
  { type: 'wait_for_device_state', icon: <Clock className="h-4 w-4" />, label: '等待状态', category: 'device' },
  { type: 'condition', icon: <Code className="h-4 w-4" />, label: '条件分支', category: 'logic' },
  { type: 'delay', icon: <Clock className="h-4 w-4" />, label: '延迟', category: 'logic' },
  { type: 'parallel', icon: <Code className="h-4 w-4" />, label: '并行执行', category: 'logic' },
  { type: 'send_alert', icon: <Bell className="h-4 w-4" />, label: '发送告警', category: 'action' },
  { type: 'log', icon: <FileText className="h-4 w-4" />, label: '记录日志', category: 'action' },
  { type: 'http_request', icon: <Code className="h-4 w-4" />, label: 'HTTP 请求', category: 'action' },
]

// Trigger type definitions
const TRIGGER_TYPES: Array<{
  type: WorkflowTriggerType
  icon: React.ReactNode
  label: string
}> = [
  { type: 'manual', icon: <Play className="h-4 w-4" />, label: '手动触发' },
  { type: 'cron', icon: <Clock className="h-4 w-4" />, label: '定时触发' },
  { type: 'event', icon: <AlertTriangle className="h-4 w-4" />, label: '事件触发' },
  { type: 'device', icon: <Zap className="h-4 w-4" />, label: '设备触发' },
]

// AI examples for workflow
const AI_EXAMPLES = [
  '每天早上8点检查温度，超过30度发送告警',
  '设备离线时自动重启，失败后通知管理员',
  '收到传感器数据后，先处理数据，然后保存到数据库',
  '并行查询多个设备状态，汇总后发送报告',
]

// ============================================================================
// Workflow Visualization Component
// ============================================================================

interface WorkflowVisualizationProps {
  steps: WorkflowStep[]
  triggers: WorkflowTrigger[]
  hasAiInput: boolean
}

function WorkflowVisualization({ steps, triggers, hasAiInput }: WorkflowVisualizationProps) {
  const getStepIcon = (type: WorkflowStepType) => {
    return STEP_TYPES.find((s) => s.type === type)?.icon || <Braces className="h-4 w-4" />
  }

  const getTriggerIcon = (type: WorkflowTriggerType) => {
    return TRIGGER_TYPES.find((t) => t.type === type)?.icon || <Clock className="h-4 w-4" />
  }

  return (
    <div className="h-full flex items-center justify-center p-8">
      <div className="max-w-lg w-full">
        {/* Flow diagram */}
        <div className="flex flex-col items-center gap-3">
          {/* Triggers node */}
          <div className="flex items-center gap-2">
            <div className="w-12 h-12 rounded-full bg-yellow-100 dark:bg-yellow-900/30 border-2 border-yellow-300 dark:border-yellow-700 flex items-center justify-center">
              <Play className="h-5 w-5 text-yellow-500" />
            </div>
            <div className="text-sm">
              <div className="font-medium">触发器</div>
              <div className="text-xs text-muted-foreground">{triggers.length} 个</div>
            </div>
          </div>

          {/* Trigger badges */}
          <div className="flex flex-wrap gap-2 justify-center">
            {triggers.slice(0, 2).map((trigger, i) => (
              <Badge key={i} variant="outline" className="text-xs">
                {getTriggerIcon(trigger.type)}
                <span className="ml-1">{trigger.type}</span>
              </Badge>
            ))}
            {triggers.length > 2 && (
              <Badge variant="outline" className="text-xs">
                +{triggers.length - 2}
              </Badge>
            )}
          </div>

          {/* Arrow */}
          <div className="w-0.5 h-4 bg-gradient-to-b from-yellow-300 to-blue-300 dark:from-yellow-700 dark:to-blue-700" />

          {/* Steps section */}
          <div className={cn(
            "w-full transition-opacity duration-300",
            steps.length === 0 && "opacity-40"
          )}>
            <div className="text-center text-sm mb-2 text-muted-foreground">
              执行步骤 ({steps.length})
            </div>

            {/* Step nodes */}
            {steps.length === 0 ? (
              <div className="text-center py-4 border-2 border-dashed rounded-lg">
                <Braces className="h-8 w-8 mx-auto mb-2 opacity-50" />
                <p className="text-xs text-muted-foreground">添加工作流步骤</p>
              </div>
            ) : (
              <div className="space-y-2">
                {steps.slice(0, 5).map((step, i) => (
                  <div key={step.id} className="flex items-center gap-2">
                    <div className={cn(
                      "w-10 h-10 rounded-full border-2 flex items-center justify-center text-xs font-medium",
                      i === 0
                        ? "bg-green-100 dark:bg-green-900/30 border-green-300 dark:border-green-700"
                        : "bg-blue-100 dark:bg-blue-900/30 border-blue-300 dark:border-blue-700"
                    )}>
                      {i + 1}
                    </div>
                    <div className="flex-1 flex items-center gap-2 p-2 bg-muted/30 rounded-md">
                      {getStepIcon(step.type)}
                      <span className="text-xs truncate">
                        {STEP_TYPES.find(s => s.type === step.type)?.label || step.type}
                      </span>
                    </div>
                  </div>
                ))}

                {/* More steps indicator */}
                {steps.length > 5 && (
                  <div className="text-center py-1">
                    <Badge variant="secondary" className="text-xs">
                      还有 {steps.length - 5} 个步骤...
                    </Badge>
                  </div>
                )}

                {/* End node */}
                <div className="flex items-center gap-2 pt-2">
                  <div className="w-10 h-10 rounded-full bg-gray-100 dark:bg-gray-800 border-2 border-gray-300 dark:border-gray-600 flex items-center justify-center">
                    <Bell className="h-4 w-4 text-gray-500" />
                  </div>
                  <div className="text-xs text-muted-foreground">完成</div>
                </div>
              </div>
            )}
          </div>
        </div>

        {/* Status text */}
        <div className="mt-6 text-center">
          <p className="text-sm text-muted-foreground">
            {hasAiInput
              ? '✨ 已输入 AI 描述，点击"AI 生成"自动配置工作流'
              : steps.length > 0
                ? '✨ 工作流配置完成'
                : '⚠️ 请添加工作流步骤'}
          </p>
        </div>
      </div>
    </div>
  )
}

// ============================================================================
// Main Component
// ============================================================================

export function WorkflowBuilderSplit({
  open,
  onClose,
  workflow,
  onSave,
  resources = { devices: [], metrics: [], alertChannels: [] },
}: WorkflowBuilderProps) {
  const [name, setName] = useState(workflow?.name || '')
  const [description, setDescription] = useState(workflow?.description || '')
  const [enabled, setEnabled] = useState(workflow?.enabled ?? true)
  const [steps, setSteps] = useState<WorkflowStep[]>((workflow?.steps as WorkflowStep[]) || [])
  const [triggers, setTriggers] = useState<WorkflowTrigger[]>((workflow?.triggers as WorkflowTrigger[]) || [{ type: 'manual', id: 'trigger-manual' }])
  const [timeout, setTimeout] = useState(300)

  // AI state
  const [aiText, setAiText] = useState('')
  const [generating, setGenerating] = useState(false)
  const [generatedCode, setGeneratedCode] = useState('')

  // Generate code preview
  const generateWorkflowCodePreview = useCallback(() => {
    if (steps.length === 0) return ''

    let code = `// Workflow: ${name || 'Unnamed Workflow'}\n\n`
    code += `// Triggers\n`
    triggers.forEach((trigger, i) => {
      code += `trigger[${i}] { type: "${trigger.type}" }\n`
    })
    code += `\n// Steps\n`
    steps.forEach((step, i) => {
      code += `step[${i}] { type: "${step.type}"${step.id ? `, id: "${step.id}"` : ''} }\n`
    })

    return code
  }, [name, steps, triggers])

  // Update code preview when data changes
  const updateCodePreview = useCallback(() => {
    setGeneratedCode(generateWorkflowCodePreview())
  }, [generateWorkflowCodePreview])

  // Add step
  const addStep = useCallback((type: WorkflowStepType) => {
    const newStep: WorkflowStep = {
      id: `step-${Date.now()}`,
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
      case 'send_command':
        ;(newStep as any).device_id = ''
        ;(newStep as any).command = ''
        ;(newStep as any).parameters = {}
        break
    }

    setSteps(prev => [...prev, newStep])
    updateCodePreview()
  }, [updateCodePreview])

  // Edit step (placeholder for dialog)
  const editStep = useCallback((index: number) => {
    // Dialog would open here in full implementation
    console.log('Edit step:', index, steps[index])
  }, [steps])

  // Delete step
  const deleteStep = useCallback((index: number) => {
    setSteps(prev => prev.filter((_, i) => i !== index))
    updateCodePreview()
  }, [updateCodePreview])

  // Move step up
  const moveStepUp = useCallback((index: number) => {
    if (index === 0) return
    setSteps(prev => {
      const newSteps = [...prev]
      ;[newSteps[index - 1], newSteps[index]] = [newSteps[index], newSteps[index - 1]]
      return newSteps
    })
    updateCodePreview()
  }, [updateCodePreview])

  // Move step down
  const moveStepDown = useCallback((index: number) => {
    setSteps(prev => {
      const newSteps = [...prev]
      if (index >= newSteps.length - 1) return newSteps
      ;[newSteps[index], newSteps[index + 1]] = [newSteps[index + 1], newSteps[index]]
      return newSteps
    })
    updateCodePreview()
  }, [updateCodePreview])

  // Add trigger
  const addTrigger = useCallback((type: WorkflowTriggerType) => {
    const newTrigger: WorkflowTrigger = {
      id: `trigger-${Date.now()}`,
      type,
    } as WorkflowTrigger

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

    setTriggers(prev => [...prev, newTrigger])
  }, [])

  // Delete trigger
  const deleteTrigger = useCallback((index: number) => {
    if (triggers.length > 1) {
      setTriggers(prev => prev.filter((_, i) => i !== index))
    }
  }, [triggers.length])

  // AI Generation
  const handleGenerateCode = useCallback(async () => {
    if (!aiText.trim()) return

    setGenerating(true)

    // Simulate AI processing
    await new Promise<void>(resolve => window.setTimeout(resolve, 800))

    const text = aiText.toLowerCase()

    // Detect trigger type
    if (text.includes('每天') || text.includes('定时') || text.includes('早上')) {
      setTriggers([{ type: 'cron', id: 'trigger-cron', expression: '0 8 * * *' }])
    } else if (text.includes('设备') && (text.includes('离线') || text.includes('状态'))) {
      setTriggers([{ type: 'device', id: 'trigger-device', device_id: '', metric: 'status', condition: '==' }])
    } else {
      setTriggers([{ type: 'manual', id: 'trigger-manual' }])
    }

    // Detect steps
    const newSteps: WorkflowStep[] = []

    if (text.includes('检查') || text.includes('查询')) {
      newSteps.push({
        id: 'step-1',
        type: 'device_query',
        device_id: resources.devices[0]?.id || '',
        metric: 'temperature',
      } as WorkflowStep)
    }

    if (text.includes('超过') || text.includes('低于') || text.includes('条件')) {
      newSteps.push({
        id: 'step-2',
        type: 'condition',
        condition: 'true',
        then_steps: [],
        else_steps: [],
      } as WorkflowStep)
    }

    if (text.includes('告警') || text.includes('通知')) {
      newSteps.push({
        id: 'step-3',
        type: 'send_alert',
        severity: 'warning',
        title: '工作流告警',
        message: '条件已触发',
      } as WorkflowStep)
    }

    if (text.includes('重启') || text.includes('命令')) {
      newSteps.push({
        id: 'step-4',
        type: 'send_command',
        device_id: resources.devices[0]?.id || '',
        command: 'restart',
        parameters: {},
      } as WorkflowStep)
    }

    if (text.includes('日志') || text.includes('记录')) {
      newSteps.push({
        id: 'step-5',
        type: 'log',
        level: 'info',
        message: '工作流已执行',
      } as WorkflowStep)
    }

    if (text.includes('延迟') || text.includes('等待')) {
      newSteps.push({
        id: 'step-6',
        type: 'delay',
        duration_seconds: 5,
      } as WorkflowStep)
    }

    setSteps(newSteps)

    // Auto-fill name if empty
    if (!name.trim()) {
      setName(aiText.slice(0, 30))
    }

    // Generate code preview
    setGeneratedCode(generateWorkflowCodePreview())

    setGenerating(false)
  }, [aiText, name, resources.devices, generateWorkflowCodePreview])

  // Validation
  const isValid = Boolean(name.trim() && steps.length > 0)

  // Save
  const handleSave = async () => {
    if (!isValid) return

    await onSave({
      name,
      description,
      enabled,
      steps,
      triggers,
      timeout_seconds: timeout,
    })
  }

  // ============================================================================
  // Left Panel Content
  // ============================================================================
  const leftPanelContent = (
    <div className="space-y-4">
      {/* Basic Info */}
      <FormSection title="基本信息" defaultExpanded={true}>
        <div className="space-y-3">
          <div>
            <Label htmlFor="workflow-name">名称 *</Label>
            <Input
              id="workflow-name"
              value={name}
              onChange={e => setName(e.target.value)}
              placeholder="例如：多步骤设备控制流程"
              className="mt-1.5"
            />
          </div>
          <div className="flex items-center gap-2">
            <Switch checked={enabled} onCheckedChange={setEnabled} id="workflow-enabled" />
            <Label htmlFor="workflow-enabled" className="text-sm cursor-pointer">
              启用工作流
            </Label>
          </div>
          <div>
            <Label htmlFor="workflow-description">描述</Label>
            <Textarea
              id="workflow-description"
              value={description}
              onChange={e => setDescription(e.target.value)}
              placeholder="描述这个工作流的功能和用途"
              rows={2}
              className="mt-1.5 resize-none"
            />
          </div>
        </div>
      </FormSection>

      {/* Triggers */}
      <FormSection title="触发器" description="定义何时启动此工作流" defaultExpanded={true}>
        <div className="space-y-3">
          <div className="grid grid-cols-2 gap-2">
            {TRIGGER_TYPES.map(triggerType => (
              <Button
                key={triggerType.type}
                variant="outline"
                size="sm"
                onClick={() => addTrigger(triggerType.type)}
                className="flex items-center gap-2 h-9"
              >
                {triggerType.icon}
                <span className="text-xs">{triggerType.label}</span>
              </Button>
            ))}
          </div>

          {/* Triggers list */}
          <div className="space-y-2">
            {triggers.map((trigger, i) => (
              <div key={trigger.id} className="flex items-center gap-2 p-2 bg-muted/40 rounded-md">
                {TRIGGER_TYPES.find(t => t.type === trigger.type)?.icon}
                <span className="text-xs flex-1">
                  {TRIGGER_TYPES.find(t => t.type === trigger.type)?.label}
                  {(trigger as any).expression && (
                    <span className="text-muted-foreground ml-1">: {(trigger as any).expression}</span>
                  )}
                  {(trigger as any).event_type && (
                    <span className="text-muted-foreground ml-1">: {(trigger as any).event_type}</span>
                  )}
                </span>
                {triggers.length > 1 && (
                  <Button
                    variant="ghost"
                    size="icon"
                    className="h-7 w-7"
                    onClick={() => deleteTrigger(i)}
                  >
                    ×
                  </Button>
                )}
              </div>
            ))}
          </div>
        </div>
      </FormSection>

      {/* Steps */}
      <FormSection title="工作流步骤" description="添加和管理工作流执行步骤" defaultExpanded={true}>
        <div className="space-y-3">
          {/* Step type selector */}
          <div className="grid grid-cols-3 gap-2">
            {STEP_TYPES.map(stepType => (
              <Button
                key={stepType.type}
                variant="outline"
                size="sm"
                onClick={() => addStep(stepType.type)}
                className="flex items-center gap-1 h-9 px-2"
                title={stepType.label}
              >
                {stepType.icon}
                <span className="text-xs truncate">{stepType.label}</span>
              </Button>
            ))}
          </div>

          {/* Steps list */}
          {steps.length === 0 ? (
            <div className="text-center py-6 text-muted-foreground border-2 border-dashed rounded-lg">
              <Braces className="h-8 w-8 mx-auto mb-2 opacity-50" />
              <p className="text-xs">暂无步骤，点击上方按钮添加</p>
            </div>
          ) : (
            <div className="space-y-2">
              {steps.map((step, i) => (
                <div key={step.id} className="flex items-center gap-2 p-2 bg-muted/30 rounded-md">
                  <GripVertical className="h-4 w-4 text-muted-foreground cursor-move" />
                  <span className="text-xs text-muted-foreground w-5">{i + 1}</span>
                  <div className="flex-1 flex items-center gap-1">
                    {STEP_TYPES.find(s => s.type === step.type)?.icon}
                    <span className="text-xs truncate">
                      {STEP_TYPES.find(s => s.type === step.type)?.label || step.type}
                    </span>
                  </div>
                  <div className="flex items-center gap-1">
                    <Button
                      variant="ghost"
                      size="icon"
                      className="h-6 w-6"
                      onClick={() => moveStepUp(i)}
                      disabled={i === 0}
                    >
                      <ArrowUp className="h-3 w-3" />
                    </Button>
                    <Button
                      variant="ghost"
                      size="icon"
                      className="h-6 w-6"
                      onClick={() => moveStepDown(i)}
                      disabled={i === steps.length - 1}
                    >
                      <ArrowDown className="h-3 w-3" />
                    </Button>
                    <Button
                      variant="ghost"
                      size="icon"
                      className="h-6 w-6"
                      onClick={() => editStep(i)}
                    >
                      <Edit3 className="h-3 w-3" />
                    </Button>
                    <Button
                      variant="ghost"
                      size="icon"
                      className="h-6 w-6 text-destructive"
                      onClick={() => deleteStep(i)}
                    >
                      <Trash2 className="h-3 w-3" />
                    </Button>
                  </div>
                </div>
              ))}
            </div>
          )}
        </div>
      </FormSection>

      {/* Settings */}
      <FormSection title="设置" defaultExpanded={false}>
        <div className="space-y-3">
          <div>
            <Label htmlFor="timeout">超时时间（秒）</Label>
            <Input
              id="timeout"
              type="number"
              value={timeout}
              onChange={e => setTimeout(parseInt(e.target.value) || 300)}
              min={1}
              max={3600}
              className="mt-1.5"
            />
            <p className="text-xs text-muted-foreground mt-1">
              工作流执行的最长时间，超时后将自动终止
            </p>
          </div>
        </div>
      </FormSection>

      {/* AI Generation */}
      <FormSection title="AI 生成" description="用自然语言描述工作流，AI 自动配置" defaultExpanded={false}>
        <div className="space-y-3">
          <Textarea
            value={aiText}
            onChange={e => setAiText(e.target.value)}
            placeholder="例如：每天早上8点检查温度，超过30度发送告警"
            rows={3}
            className="resize-none"
          />
          <div className="flex flex-wrap gap-2">
            {AI_EXAMPLES.map(ex => (
              <Button
                key={ex}
                variant="outline"
                size="sm"
                type="button"
                onClick={() => setAiText(ex)}
                className="h-7 text-xs"
              >
                {ex.slice(0, 10)}...
              </Button>
            ))}
          </div>
        </div>
      </FormSection>
    </div>
  )

  return (
    <SplitPaneBuilder
      open={open}
      onClose={onClose}
      title={workflow ? '编辑工作流' : '创建工作流'}
      description="定义复杂的多步骤自动化流程，支持条件分支、并行执行等高级功能"
      icon={<Sparkles className="h-5 w-5 text-blue-500" />}
      badge={
        <Badge variant={enabled ? 'default' : 'secondary'} className="text-xs">
          {enabled ? '启用' : '禁用'}
        </Badge>
      }
      leftPanel={{
        title: '配置',
        content: leftPanelContent,
      }}
      rightPanel={{
        visualization: (
          <WorkflowVisualization
            steps={steps}
            triggers={triggers}
            hasAiInput={aiText.trim().length > 0}
          />
        ),
        code: generatedCode || undefined,
        codeLanguage: 'javascript',
        loading: generating,
      }}
      isValid={isValid}
      isSaving={false}
      saveLabel="保存"
      onSave={handleSave}
      onGenerate={handleGenerateCode}
      generating={generating}
      generateLabel="AI 生成工作流"
    />

    // Note: Step/Trigger config dialogs would be rendered here
    // For simplicity, we're using a basic inline edit approach
  )
}
