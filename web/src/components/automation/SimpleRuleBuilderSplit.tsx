import { useState, useEffect, useCallback } from 'react'
import { useTranslation } from 'react-i18next'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Textarea } from '@/components/ui/textarea'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import { Badge } from '@/components/ui/badge'
import { Zap, Calendar, Globe, Play, Bell, FileText, Sparkles, Lightbulb } from 'lucide-react'
import type { Rule, RuleTrigger, RuleCondition, RuleAction, DeviceType } from '@/types'
import {
  ThreePaneBuilder,
  FormSection,
} from './ThreePaneBuilder'
import { cn } from '@/lib/utils'

interface RuleBuilderProps {
  open: boolean
  onOpenChange: (open: boolean) => void
  rule?: Rule
  onSave: (rule: Partial<Rule>) => Promise<void>
  resources?: {
    devices: Array<{ id: string; name: string; device_type?: string }>
    deviceTypes?: DeviceType[]
  }
}

type TriggerType = 'device_state' | 'schedule' | 'manual' | 'event'

type ScheduleType = 'periodic' | 'once' | 'interval'

type PeriodicType = 'daily' | 'weekdays' | 'weekly' | 'monthly'

type IntervalUnit = 'minutes' | 'hours' | 'days'

const OPERATORS = [
  { value: '>', label: '大于', symbol: '>' },
  { value: '<', label: '小于', symbol: '<' },
  { value: '>=', label: '大于等于', symbol: '≥' },
  { value: '<=', label: '小于等于', symbol: '≤' },
  { value: '==', label: '等于', symbol: '=' },
  { value: '!=', label: '不等于', symbol: '≠' },
]

const EVENT_TYPES = [
  'DeviceOnline',
  'DeviceOffline',
  'DeviceMetric',
  'RuleTriggered',
  'WorkflowCompleted',
  'LlmDecisionProposed',
  'AlertCreated',
]

// ============================================================================
// Helper Functions
// ============================================================================

function getDeviceType(
  deviceId: string,
  devices: Array<{ id: string; name: string; device_type?: string }>,
  deviceTypes?: DeviceType[]
): string {
  const device = devices.find(d => d.id === deviceId)
  return device?.device_type || deviceTypes?.[0]?.device_type || ''
}

function getDeviceMetrics(
  deviceId: string,
  devices: Array<{ id: string; name: string; device_type?: string }>,
  deviceTypes?: DeviceType[]
): Array<{ name: string; display_name?: string }> {
  const deviceTypeName = getDeviceType(deviceId, devices, deviceTypes)
  const deviceType = deviceTypes?.find(t => t.device_type === deviceTypeName)
  return deviceType?.metrics || []
}

function getDeviceCommands(
  deviceId: string,
  devices: Array<{ id: string; name: string; device_type?: string }>,
  deviceTypes?: DeviceType[]
): Array<{ name: string; display_name?: string }> {
  const deviceTypeName = getDeviceType(deviceId, devices, deviceTypes)
  const deviceType = deviceTypes?.find(t => t.device_type === deviceTypeName)
  return deviceType?.commands || []
}

// ============================================================================
// Rule Visualization Component
// ============================================================================

interface RuleVisualizationProps {
  triggerType: TriggerType
  conditions: RuleCondition[]
  actions: RuleAction[]
  triggerState?: string
  triggerCron?: string
  scheduleTime?: string
  scheduleDays?: number[]
  scheduleType?: ScheduleType
  periodicType?: PeriodicType
  scheduleDayOfMonth?: number
  onceDateTime?: string
  intervalValue?: number
  intervalUnit?: IntervalUnit
  hasAiInput: boolean
}

function RuleVisualization({
  triggerType,
  conditions,
  actions,
  triggerState,
  triggerCron,
  scheduleTime,
  scheduleDays,
  scheduleType,
  periodicType,
  scheduleDayOfMonth,
  onceDateTime,
  intervalValue,
  intervalUnit,
  hasAiInput,
}: RuleVisualizationProps) {
  const getTriggerIcon = () => {
    switch (triggerType) {
      case 'device_state': return <Zap className="h-5 w-5 text-yellow-500" />
      case 'schedule': return <Calendar className="h-5 w-5 text-blue-500" />
      case 'event': return <Globe className="h-5 w-5 text-green-500" />
      case 'manual': return <Play className="h-5 w-5 text-gray-500" />
    }
  }

  const getTriggerLabel = () => {
    switch (triggerType) {
      case 'device_state': return '设备状态'
      case 'schedule': return '定时触发'
      case 'event': return '事件触发'
      case 'manual': return '手动触发'
    }
  }

  const getTriggerDetail = () => {
    switch (triggerType) {
      case 'device_state':
        return triggerState ? `状态: ${triggerState}` : '配置设备状态触发'
      case 'schedule':
        if (scheduleType === 'once' && onceDateTime) {
          return `单次: ${new Date(onceDateTime).toLocaleString('zh-CN')}`
        }
        if (scheduleType === 'interval' && intervalValue) {
          const unitText = intervalUnit === 'minutes' ? '分钟' : intervalUnit === 'hours' ? '小时' : '天'
          return `间隔: 每 ${intervalValue} ${unitText}`
        }
        if (scheduleType === 'periodic' && scheduleTime) {
          if (periodicType === 'daily') return `每天 ${scheduleTime}`
          if (periodicType === 'weekdays') return `工作日 ${scheduleTime}`
          if (periodicType === 'weekly' && scheduleDays && scheduleDays.length > 0) {
            const days = scheduleDays.map(d => ['一', '二', '三', '四', '五', '六', '日'][d - 1]).join('、')
            return `每周 ${days} ${scheduleTime}`
          }
          if (periodicType === 'monthly' && scheduleDayOfMonth) {
            return `每月${scheduleDayOfMonth}号 ${scheduleTime}`
          }
        }
        return triggerCron ? `Cron: ${triggerCron}` : '配置定时规则'
      case 'event':
        return '选择事件类型'
      case 'manual':
        return '手动执行'
    }
  }

  return (
    <div className="h-full flex items-center justify-center p-8">
      <div className="max-w-lg w-full">
        {/* Flow diagram */}
        <div className="flex flex-col items-center gap-4">
          {/* Trigger node */}
          <div className="flex items-center gap-3">
            <div className="w-14 h-14 rounded-full bg-yellow-100 dark:bg-yellow-900/30 border-2 border-yellow-300 dark:border-yellow-700 flex items-center justify-center">
              {getTriggerIcon()}
            </div>
            <div className="text-sm">
              <div className="font-medium">触发器</div>
              <div className="text-xs text-muted-foreground">{getTriggerLabel()}</div>
            </div>
          </div>

          {/* Trigger detail badge */}
          <Badge variant="outline" className="text-xs">
            {getTriggerDetail()}
          </Badge>

          {/* Arrow to conditions */}
          {triggerType === 'device_state' && (
            <>
              <div className="w-0.5 h-6 bg-gradient-to-b from-yellow-300 to-blue-300 dark:from-yellow-700 dark:to-blue-700" />

              {/* Conditions node */}
              <div className={cn(
                "flex items-center gap-3 transition-opacity duration-300",
                conditions.length === 0 && "opacity-40"
              )}>
                <div className="w-14 h-14 rounded-full bg-blue-100 dark:bg-blue-900/30 border-2 border-blue-300 dark:border-blue-700 flex items-center justify-center">
                  <Lightbulb className="h-5 w-5 text-blue-500" />
                </div>
                <div className="text-sm">
                  <div className="font-medium">条件</div>
                  <div className="text-xs text-muted-foreground">
                    {conditions.length > 0
                      ? `${conditions.length} 个条件`
                      : '添加触发条件'}
                  </div>
                </div>
              </div>
            </>
          )}

          {/* Arrow to actions */}
          <div className="w-0.5 h-6 bg-gradient-to-b from-blue-300 to-green-300 dark:from-blue-700 dark:to-green-700" />

          {/* Actions node */}
          <div className={cn(
            "flex items-center gap-3 transition-opacity duration-300",
            actions.length === 0 && "opacity-40"
          )}>
            <div className="w-14 h-14 rounded-full bg-green-100 dark:bg-green-900/30 border-2 border-green-300 dark:border-green-700 flex items-center justify-center">
              <Play className="h-5 w-5 text-green-500" />
            </div>
            <div className="text-sm">
              <div className="font-medium">动作</div>
              <div className="text-xs text-muted-foreground">
                {actions.length > 0
                  ? `${actions.length} 个动作`
                  : '添加执行动作'}
              </div>
            </div>
          </div>

          {/* Action badges preview */}
          {actions.length > 0 && (
            <div className="flex flex-wrap gap-2 justify-center mt-2">
              {actions.slice(0, 3).map((action, i) => (
                <Badge key={i} variant="secondary" className="text-xs">
                  {action.type === 'Execute' && <Zap className="h-3 w-3 mr-1" />}
                  {action.type === 'Notify' && <Bell className="h-3 w-3 mr-1" />}
                  {action.type === 'Log' && <FileText className="h-3 w-3 mr-1" />}
                  {action.type === 'Execute' ? '执行' : action.type === 'Notify' ? '通知' : '日志'}
                </Badge>
              ))}
              {actions.length > 3 && (
                <Badge variant="secondary" className="text-xs">
                  +{actions.length - 3}
                </Badge>
              )}
            </div>
          )}
        </div>

        {/* Status text */}
        <div className="mt-8 text-center">
          <p className="text-sm text-muted-foreground">
            {hasAiInput
              ? '✨ AI 正在处理您的需求，配置将自动更新'
              : actions.length > 0
                ? '✨ 规则配置完成'
                : '⚠️ 请在右侧配置触发器和动作'}
          </p>
        </div>
      </div>
    </div>
  )
}

// ============================================================================
// Main Component
// ============================================================================

export function SimpleRuleBuilderSplit({
  open,
  onOpenChange,
  rule,
  onSave,
  resources = { devices: [], deviceTypes: [] },
}: RuleBuilderProps) {
  const { t } = useTranslation(['automation', 'common'])

  // Basic info
  const [name, setName] = useState('')
  const [description, setDescription] = useState('')

  // Trigger state
  const [triggerType, setTriggerType] = useState<TriggerType>('manual')
  const [triggerDeviceId, setTriggerDeviceId] = useState('')
  const [triggerState, setTriggerState] = useState('')
  const [triggerCron, setTriggerCron] = useState('0 * * * *')

  // Schedule state
  const [scheduleType, setScheduleType] = useState<ScheduleType>('periodic')
  const [periodicType, setPeriodicType] = useState<PeriodicType>('daily')
  const [scheduleTime, setScheduleTime] = useState('08:00') // HH:mm format
  const [scheduleDays, setScheduleDays] = useState<number[]>([1, 2, 3, 4, 5]) // Days of week (1-7)
  const [scheduleDayOfMonth, setScheduleDayOfMonth] = useState(1) // Day of month (1-31)
  const [onceDateTime, setOnceDateTime] = useState('') // ISO datetime string
  const [intervalValue, setIntervalValue] = useState(5) // Interval number
  const [intervalUnit, setIntervalUnit] = useState<IntervalUnit>('minutes')

  // Event state
  const [triggerEventType, setTriggerEventType] = useState('')
  const [triggerEventFilters, setTriggerEventFilters] = useState('')

  // Conditions state
  const [conditions, setConditions] = useState<RuleCondition[]>([])

  // Actions state
  const [actions, setActions] = useState<RuleAction[]>([])

  // AI state
  const [aiGenerating, setAiGenerating] = useState(false)
  const [aiProcessed, setAiProcessed] = useState(false)

  // ============================================================================
  // Initialize form from rule
  // ============================================================================
  useEffect(() => {
    if (open && rule) {
      setName(rule.name || '')
      setDescription(rule.description || '')

      if (rule.trigger) {
        setTriggerType(rule.trigger.type as TriggerType)
        switch (rule.trigger.type) {
          case 'device_state':
            setTriggerDeviceId(rule.trigger.device_id || '')
            setTriggerState(rule.trigger.state || '')
            break
          case 'schedule':
            setTriggerCron(rule.trigger.cron || '0 * * * *')
            break
          case 'event':
            setTriggerEventType(rule.trigger.event_type || '')
            setTriggerEventFilters(JSON.stringify(rule.trigger.filters || {}, null, 2))
            break
          case 'manual':
            break
        }
      }

      if (rule.condition) {
        setConditions([rule.condition])
      } else {
        setConditions([])
      }

      if (rule.actions && rule.actions.length > 0) {
        setActions(rule.actions)
      } else {
        setActions([])
      }
    } else if (open) {
      resetForm()
    }
  }, [open, rule])

  const resetForm = useCallback(() => {
    setName('')
    setDescription('')
    setTriggerType('manual')
    setTriggerDeviceId('')
    setTriggerState('')
    setTriggerCron('0 * * * *')
    setScheduleType('periodic')
    setPeriodicType('daily')
    setScheduleTime('08:00')
    setScheduleDays([1, 2, 3, 4, 5])
    setScheduleDayOfMonth(1)
    setOnceDateTime('')
    setIntervalValue(5)
    setIntervalUnit('minutes')
    setTriggerEventType('')
    setTriggerEventFilters('')
    setConditions([])
    setActions([{ type: 'Log', level: 'info', message: t('automation:logMessage', { defaultValue: '规则已触发' }) }])
    setAiProcessed(false)
  }, [t])

  // ============================================================================
  // Trigger helpers
  // ============================================================================

  // Convert schedule config to Cron expression
  const generateCronExpression = useCallback((): string => {
    if (scheduleType === 'once') {
      // For one-time, use a special format or return empty
      // We'll store the actual datetime separately
      return onceDateTime ? `@${new Date(onceDateTime).getTime()}` : ''
    }

    if (scheduleType === 'interval') {
      // Interval format: */N * * * * for minutes, 0 */N * * * for hours, etc.
      switch (intervalUnit) {
        case 'minutes':
          return `*/${intervalValue} * * * *`
        case 'hours':
          return `0 */${intervalValue} * * *`
        case 'days':
          return `${scheduleTime.split(':')[1]} ${scheduleTime.split(':')[0]} */${intervalValue} * *`
      }
    }

    // Periodic schedules
    const [hours, minutes] = scheduleTime.split(':').map(Number)

    switch (periodicType) {
      case 'daily':
        return `${minutes} ${hours} * * *`
      case 'weekdays':
        // Mon-Fri: 1-5 in cron (0=Sun, 6=Sat)
        return `${minutes} ${hours} * * 1-5`
      case 'weekly':
        // Days: 1=Mon, 2=Tue, 3=Wed, 4=Thu, 5=Fri, 6=Sat, 7=Sun
        // Cron: 0=Sun, 1=Mon, ..., 6=Sat
        const cronDays = scheduleDays.map(d => d === 7 ? 0 : d).join(',')
        return `${minutes} ${hours} * * ${cronDays}`
      case 'monthly':
        // Day of month (1-31)
        return `${minutes} ${hours} ${scheduleDayOfMonth} * *`
    }
  }, [scheduleType, periodicType, scheduleTime, scheduleDays, scheduleDayOfMonth, onceDateTime, intervalValue, intervalUnit])

  const buildTrigger = useCallback((): RuleTrigger | undefined => {
    switch (triggerType) {
      case 'device_state':
        if (!triggerDeviceId || !triggerState) return undefined
        return { type: 'device_state', device_id: triggerDeviceId, state: triggerState }
      case 'schedule':
        const cron = generateCronExpression()
        return { type: 'schedule', cron }
      case 'event':
        if (!triggerEventType) return undefined
        let filters: Record<string, unknown> | undefined = undefined
        if (triggerEventFilters.trim()) {
          try {
            filters = JSON.parse(triggerEventFilters)
          } catch {
            // Invalid JSON, ignore filters
          }
        }
        return { type: 'event', event_type: triggerEventType, filters }
      case 'manual':
        return { type: 'manual' }
    }
  }, [triggerType, triggerDeviceId, triggerState, triggerEventType, triggerEventFilters, generateCronExpression])

  // ============================================================================
  // Condition helpers
  // ============================================================================

  const createDefaultCondition = useCallback((): RuleCondition => {
    const firstDevice = resources.devices[0]
    if (!firstDevice) {
      return { device_id: '', metric: 'temperature', operator: '>', threshold: 30 }
    }
    const metrics = getDeviceMetrics(firstDevice.id, resources.devices, resources.deviceTypes)
    return {
      device_id: firstDevice.id,
      metric: metrics[0]?.name || 'temperature',
      operator: '>',
      threshold: 30,
    }
  }, [resources.devices, resources.deviceTypes])

  const addCondition = useCallback(() => {
    setConditions(prev => [...prev, createDefaultCondition()])
  }, [createDefaultCondition])

  const updateCondition = useCallback((index: number, data: Partial<RuleCondition>) => {
    setConditions(prev => prev.map((c, i) => (i === index ? { ...c, ...data } : c)))
  }, [])

  const removeCondition = useCallback((index: number) => {
    setConditions(prev => prev.filter((_, i) => i !== index))
  }, [])

  // ============================================================================
  // Action helpers
  // ============================================================================

  const addAction = useCallback((type: 'Notify' | 'Execute' | 'Log') => {
    setActions(prev => {
      let newAction: RuleAction
      if (type === 'Notify') {
        newAction = { type: 'Notify', message: '' }
      } else if (type === 'Execute') {
        const firstDevice = resources.devices[0]
        const commands = firstDevice ? getDeviceCommands(firstDevice.id, resources.devices, resources.deviceTypes) : []
        newAction = {
          type: 'Execute',
          device_id: firstDevice?.id || '',
          command: commands[0]?.name || 'turn_on',
          params: {},
        }
      } else {
        newAction = { type: 'Log', level: 'info', message: '' }
      }
      return [...prev, newAction]
    })
  }, [resources.devices, resources.deviceTypes])

  const updateAction = useCallback((index: number, data: Partial<RuleAction>) => {
    setActions(prev => prev.map((a, i) => {
      if (i !== index) return a
      return { ...a, ...data } as RuleAction
    }))
  }, [])

  const removeAction = useCallback((index: number) => {
    setActions(prev => prev.filter((_, i) => i !== index))
  }, [])

  // ============================================================================
  // AI Chat Handler
  // ============================================================================

  const handleAiChat = useCallback(async (message: string) => {
    setAiGenerating(true)
    setAiProcessed(false)

    // Simulate AI processing with regex-based parsing
    await new Promise<void>(resolve => window.setTimeout(resolve, 800))

    const text = message.toLowerCase()

    // Detect trigger type
    if (text.includes('每天') || text.includes('定时') || text.includes('早上') || text.includes('晚上')) {
      setTriggerType('schedule')
      setTriggerCron('0 8 * * *')
    } else if (text.includes('事件') || text.includes('event')) {
      setTriggerType('event')
      setTriggerEventType('DeviceMetric')
    } else {
      setTriggerType('device_state')
      if (resources.devices.length > 0) {
        setTriggerDeviceId(resources.devices[0].id)
        setTriggerState('active')
      }
    }

    // Detect conditions
    const newConditions: RuleCondition[] = []
    const tempMatch = message.match(/温度.*?(\d+)/)
    const humMatch = message.match(/湿度.*?(\d+)/)

    if (tempMatch && resources.devices.length > 0) {
      newConditions.push({
        device_id: resources.devices[0].id,
        metric: 'temperature',
        operator: '>',
        threshold: parseFloat(tempMatch[1]) || 30,
      })
    }
    if (humMatch && resources.devices.length > 0) {
      newConditions.push({
        device_id: resources.devices[0].id,
        metric: 'humidity',
        operator: '<',
        threshold: parseFloat(humMatch[1]) || 40,
      })
    }
    setConditions(newConditions)

    // Detect actions
    const newActions: RuleAction[] = []
    if (text.includes('开空调') || text.includes('打开空调')) {
      newActions.push({
        type: 'Execute',
        device_id: resources.devices[0]?.id || '',
        command: 'turn_on',
        params: {},
      })
    }
    if (text.includes('关空调') || text.includes('关闭空调')) {
      newActions.push({
        type: 'Execute',
        device_id: resources.devices[0]?.id || '',
        command: 'turn_off',
        params: {},
      })
    }
    if (text.includes('开灯')) {
      newActions.push({
        type: 'Execute',
        device_id: resources.devices[0]?.id || '',
        command: 'turn_on',
        params: {},
      })
    }
    if (text.includes('通知') || text.includes('发送')) {
      newActions.push({ type: 'Notify', message: '规则已触发' })
    }
    if (text.includes('记录')) {
      newActions.push({ type: 'Log', level: 'info', message: '规则已执行' })
    }

    if (newActions.length === 0) {
      newActions.push({ type: 'Log', level: 'info', message: '规则已触发' })
    }
    setActions(newActions)

    // Auto-fill name if empty
    if (!name.trim()) {
      setName(message.slice(0, 30))
    }

    setAiGenerating(false)
    setAiProcessed(true)
  }, [name, resources.devices])

  // ============================================================================
  // Validation
  // ============================================================================

  const isValid = Boolean(name.trim() &&
    ((triggerType === 'manual') ||
     (triggerType === 'device_state' && triggerDeviceId) ||
     (triggerType === 'schedule' && (
       (scheduleType === 'periodic') ||
       (scheduleType === 'once' && onceDateTime) ||
       (scheduleType === 'interval' && intervalValue > 0)
     )) ||
     (triggerType === 'event' && triggerEventType)))

  const getValidationMessage = () => {
    if (!name.trim()) return '请输入规则名称'
    if (triggerType === 'device_state' && !triggerDeviceId) return '请选择设备'
    if (triggerType === 'schedule') {
      if (scheduleType === 'once' && !onceDateTime) return '请选择执行时间'
      if (scheduleType === 'interval' && intervalValue <= 0) return '请输入有效的间隔时间'
      if (scheduleType === 'periodic' && periodicType === 'weekly' && scheduleDays.length === 0) return '请选择至少一天'
    }
    if (triggerType === 'event' && !triggerEventType) return '请选择事件类型'
    return ''
  }

  // ============================================================================
  // Save
  // ============================================================================

  const handleSave = async () => {
    if (!isValid) return

    const trigger = buildTrigger()
    const condition = triggerType === 'device_state' && conditions.length > 0
      ? conditions[0]
      : undefined

    await onSave({
      name,
      description,
      enabled: true, // Always create as enabled, control via list
      trigger,
      condition,
      actions: actions.length > 0 ? actions : undefined,
    })
  }

  // Device options
  const deviceOptions = resources.devices.map(d => ({ value: d.id, label: d.name }))

  // ============================================================================
  // Right Panel Content (Form)
  // ============================================================================
  const rightPanelContent = (
    <div className="space-y-4">
      {/* Basic Info */}
      <FormSection title="基本信息" defaultExpanded={true}>
        <div className="space-y-3">
          <div>
            <Label htmlFor="rule-name">名称 *</Label>
            <Input
              id="rule-name"
              value={name}
              onChange={e => setName(e.target.value)}
              placeholder="例如：温度过高自动开空调"
              className="mt-1.5"
            />
          </div>
          <div>
            <Label htmlFor="rule-description">描述</Label>
            <Input
              id="rule-description"
              value={description}
              onChange={e => setDescription(e.target.value)}
              placeholder="规则描述（可选）"
              className="mt-1.5"
            />
          </div>
        </div>
      </FormSection>

      {/* Trigger Configuration */}
      <FormSection title="触发器配置" description="选择何时触发此规则" defaultExpanded={true}>
        <div className="space-y-3">
          {/* Trigger Type Buttons */}
          <div className="grid grid-cols-4 gap-2">
            <Button
              type="button"
              variant={triggerType === 'manual' ? 'default' : 'outline'}
              onClick={() => setTriggerType('manual')}
              className="flex flex-col items-center gap-1 h-auto py-2"
            >
              <Play className="h-4 w-4" />
              <span className="text-xs">手动</span>
            </Button>
            <Button
              type="button"
              variant={triggerType === 'device_state' ? 'default' : 'outline'}
              onClick={() => setTriggerType('device_state')}
              className="flex flex-col items-center gap-1 h-auto py-2"
            >
              <Zap className="h-4 w-4" />
              <span className="text-xs">设备状态</span>
            </Button>
            <Button
              type="button"
              variant={triggerType === 'schedule' ? 'default' : 'outline'}
              onClick={() => setTriggerType('schedule')}
              className="flex flex-col items-center gap-1 h-auto py-2"
            >
              <Calendar className="h-4 w-4" />
              <span className="text-xs">定时</span>
            </Button>
            <Button
              type="button"
              variant={triggerType === 'event' ? 'default' : 'outline'}
              onClick={() => setTriggerType('event')}
              className="flex flex-col items-center gap-1 h-auto py-2"
            >
              <Globe className="h-4 w-4" />
              <span className="text-xs">事件</span>
            </Button>
          </div>

          {/* Device State Trigger Config */}
          {triggerType === 'device_state' && (
            <div className="space-y-2 pt-2 border-t">
              <div className="flex gap-2">
                <Select value={triggerDeviceId} onValueChange={setTriggerDeviceId}>
                  <SelectTrigger className="flex-1">
                    <SelectValue placeholder="选择设备" />
                  </SelectTrigger>
                  <SelectContent>
                    {deviceOptions.map(d => (
                      <SelectItem key={d.value} value={d.value}>{d.label}</SelectItem>
                    ))}
                  </SelectContent>
                </Select>
                <Input
                  value={triggerState}
                  onChange={e => setTriggerState(e.target.value)}
                  placeholder="状态值 (如: active)"
                  className="flex-1"
                />
              </div>
            </div>
          )}

          {/* Schedule Trigger Config */}
          {triggerType === 'schedule' && (
            <div className="space-y-3 pt-2 border-t">
              {/* Schedule Type Selector */}
              <div>
                <Label className="text-sm mb-2 block">定时类型</Label>
                <div className="grid grid-cols-3 gap-2">
                  <Button
                    type="button"
                    variant={scheduleType === 'periodic' ? 'default' : 'outline'}
                    size="sm"
                    onClick={() => setScheduleType('periodic')}
                  >
                    周期时间
                  </Button>
                  <Button
                    type="button"
                    variant={scheduleType === 'once' ? 'default' : 'outline'}
                    size="sm"
                    onClick={() => setScheduleType('once')}
                  >
                    单次时间
                  </Button>
                  <Button
                    type="button"
                    variant={scheduleType === 'interval' ? 'default' : 'outline'}
                    size="sm"
                    onClick={() => setScheduleType('interval')}
                  >
                    间隔时间
                  </Button>
                </div>
              </div>

              {/* Periodic Schedule */}
              {scheduleType === 'periodic' && (
                <div className="space-y-3">
                  <div>
                    <Label className="text-sm mb-2 block">周期</Label>
                    <div className="grid grid-cols-4 gap-2">
                      <Button
                        type="button"
                        variant={periodicType === 'daily' ? 'default' : 'outline'}
                        size="sm"
                        onClick={() => setPeriodicType('daily')}
                      >
                        每天
                      </Button>
                      <Button
                        type="button"
                        variant={periodicType === 'weekdays' ? 'default' : 'outline'}
                        size="sm"
                        onClick={() => setPeriodicType('weekdays')}
                      >
                        工作日
                      </Button>
                      <Button
                        type="button"
                        variant={periodicType === 'weekly' ? 'default' : 'outline'}
                        size="sm"
                        onClick={() => setPeriodicType('weekly')}
                      >
                        每周
                      </Button>
                      <Button
                        type="button"
                        variant={periodicType === 'monthly' ? 'default' : 'outline'}
                        size="sm"
                        onClick={() => setPeriodicType('monthly')}
                      >
                        每月
                      </Button>
                    </div>
                  </div>

                  {/* Time input for daily/weekdays/weekly/monthly */}
                  <div>
                    <Label className="text-sm">执行时间</Label>
                    <Input
                      type="time"
                      value={scheduleTime}
                      onChange={e => setScheduleTime(e.target.value)}
                      className="mt-1.5"
                    />
                  </div>

                  {/* Weekly day selector */}
                  {periodicType === 'weekly' && (
                    <div>
                      <Label className="text-sm mb-2 block">重复星期</Label>
                      <div className="flex flex-wrap gap-2">
                        {['一', '二', '三', '四', '五', '六', '日'].map((day, i) => (
                          <button
                            key={i}
                            type="button"
                            onClick={() => {
                              const dayNum = i + 1
                              setScheduleDays(prev =>
                                prev.includes(dayNum)
                                  ? prev.filter(d => d !== dayNum)
                                  : [...prev, dayNum]
                              )
                            }}
                            className={cn(
                              "w-9 h-9 rounded-full text-sm font-medium transition-colors",
                              scheduleDays.includes(i + 1)
                                ? "bg-purple-500 text-white"
                                : "bg-muted text-muted-foreground hover:bg-muted/70"
                            )}
                          >
                            {day}
                          </button>
                        ))}
                      </div>
                    </div>
                  )}

                  {/* Monthly day selector */}
                  {periodicType === 'monthly' && (
                    <div>
                      <Label className="text-sm mb-2 block">每月几号</Label>
                      <Select value={String(scheduleDayOfMonth)} onValueChange={v => setScheduleDayOfMonth(Number(v))}>
                        <SelectTrigger className="w-full">
                          <SelectValue />
                        </SelectTrigger>
                        <SelectContent>
                          {Array.from({ length: 31 }, (_, i) => (
                            <SelectItem key={i + 1} value={String(i + 1)}>
                              每月{i + 1}号
                            </SelectItem>
                          ))}
                        </SelectContent>
                      </Select>
                    </div>
                  )}

                  {/* Preview */}
                  <p className="text-xs text-muted-foreground">
                    {periodicType === 'daily' && `每天 ${scheduleTime} 执行`}
                    {periodicType === 'weekdays' && `工作日 ${scheduleTime} 执行`}
                    {periodicType === 'weekly' && scheduleDays.length > 0 &&
                      `每周 ${scheduleDays.map(d => ['一', '二', '三', '四', '五', '六', '日'][d - 1]).join('、')} ${scheduleTime} 执行`}
                    {periodicType === 'monthly' && `每月${scheduleDayOfMonth}号 ${scheduleTime} 执行`}
                  </p>
                </div>
              )}

              {/* One-time Schedule */}
              {scheduleType === 'once' && (
                <div>
                  <Label className="text-sm">执行时间</Label>
                  <Input
                    type="datetime-local"
                    value={onceDateTime}
                    onChange={e => setOnceDateTime(e.target.value)}
                    className="mt-1.5"
                  />
                  <p className="text-xs text-muted-foreground mt-1">
                    {onceDateTime ? `将在 ${new Date(onceDateTime).toLocaleString('zh-CN')} 执行一次` : '请选择执行时间'}
                  </p>
                </div>
              )}

              {/* Interval Schedule */}
              {scheduleType === 'interval' && (
                <div className="space-y-3">
                  <div className="flex items-center gap-2">
                    <Label className="text-sm">每隔</Label>
                    <Input
                      type="number"
                      min={1}
                      value={intervalValue}
                      onChange={e => setIntervalValue(Math.max(1, Number(e.target.value) || 1))}
                      className="w-20 h-9"
                    />
                    <Select value={intervalUnit} onValueChange={v => setIntervalUnit(v as IntervalUnit)}>
                      <SelectTrigger className="w-24 h-9">
                        <SelectValue />
                      </SelectTrigger>
                      <SelectContent>
                        <SelectItem value="minutes">分钟</SelectItem>
                        <SelectItem value="hours">小时</SelectItem>
                        <SelectItem value="days">天</SelectItem>
                      </SelectContent>
                    </Select>
                    <Label className="text-sm">执行一次</Label>
                  </div>
                  <p className="text-xs text-muted-foreground">
                    {intervalUnit === 'minutes' && `每隔 ${intervalValue} 分钟执行一次`}
                    {intervalUnit === 'hours' && `每隔 ${intervalValue} 小时执行一次`}
                    {intervalUnit === 'days' && `每隔 ${intervalValue} 天 ${scheduleTime} 执行一次`}
                  </p>
                </div>
              )}
            </div>
          )}

          {/* Event Trigger Config */}
          {triggerType === 'event' && (
            <div className="space-y-2 pt-2 border-t">
              <Select value={triggerEventType} onValueChange={setTriggerEventType}>
                <SelectTrigger>
                  <SelectValue placeholder="选择事件类型" />
                </SelectTrigger>
                <SelectContent>
                  {EVENT_TYPES.map(e => (
                    <SelectItem key={e} value={e}>{e}</SelectItem>
                  ))}
                </SelectContent>
              </Select>
              <Textarea
                value={triggerEventFilters}
                onChange={e => setTriggerEventFilters(e.target.value)}
                placeholder='{"key": "value"}'
                rows={2}
                className="font-mono text-sm"
              />
            </div>
          )}
        </div>
      </FormSection>

      {/* Conditions */}
      {triggerType === 'device_state' && (
        <FormSection
          title="触发条件"
          description="满足条件时执行动作"
        >
          <div className="space-y-3">
            <Button onClick={addCondition} variant="outline" size="sm" className="w-full">
              + 添加条件
            </Button>

            {conditions.map((cond, i) => {
              const metrics = getDeviceMetrics(cond.device_id, resources.devices, resources.deviceTypes)
              return (
                <div key={i} className="flex items-center gap-2 p-2 bg-muted/40 rounded-md">
                  <span className="text-xs text-muted-foreground w-5">{i + 1}</span>
                  <Select value={cond.device_id} onValueChange={v => updateCondition(i, {
                    device_id: v,
                    metric: getDeviceMetrics(v, resources.devices, resources.deviceTypes)[0]?.name || 'temperature'
                  })}>
                    <SelectTrigger className="w-24 h-8">
                      <SelectValue />
                    </SelectTrigger>
                    <SelectContent>
                      {deviceOptions.map(d => (
                        <SelectItem key={d.value} value={d.value}>{d.label}</SelectItem>
                      ))}
                    </SelectContent>
                  </Select>
                  <Select value={cond.metric} onValueChange={v => updateCondition(i, { metric: v })}>
                    <SelectTrigger className="w-20 h-8">
                      <SelectValue />
                    </SelectTrigger>
                    <SelectContent>
                      {metrics.map(m => (
                        <SelectItem key={m.name} value={m.name}>{m.display_name || m.name}</SelectItem>
                      ))}
                    </SelectContent>
                  </Select>
                  <Select value={cond.operator} onValueChange={v => updateCondition(i, { operator: v as any })}>
                    <SelectTrigger className="w-14 h-8">
                      <SelectValue />
                    </SelectTrigger>
                    <SelectContent>
                      {OPERATORS.map(o => (
                        <SelectItem key={o.value} value={o.value}>{o.symbol}</SelectItem>
                      ))}
                    </SelectContent>
                  </Select>
                  <Input
                    type="number"
                    value={cond.threshold}
                    onChange={e => updateCondition(i, { threshold: parseFloat(e.target.value) || 0 })}
                    placeholder="值"
                    className="w-16 h-8"
                  />
                  <Button
                    variant="ghost"
                    size="icon"
                    className="h-8 w-8 ml-auto"
                    onClick={() => removeCondition(i)}
                  >
                    ×
                  </Button>
                </div>
              )
            })}
          </div>
        </FormSection>
      )}

      {/* Actions */}
      <FormSection title="执行动作" description="触发时执行的操作" defaultExpanded={true}>
        <div className="space-y-3">
          {/* Add Action Buttons */}
          <div className="flex gap-2">
            <Button onClick={() => addAction('Execute')} variant="outline" size="sm">
              <Zap className="h-3 w-3 mr-1" />
              执行命令
            </Button>
            <Button onClick={() => addAction('Notify')} variant="outline" size="sm">
              <Bell className="h-3 w-3 mr-1" />
              发送通知
            </Button>
            <Button onClick={() => addAction('Log')} variant="outline" size="sm">
              <FileText className="h-3 w-3 mr-1" />
              记录日志
            </Button>
          </div>

          {actions.map((action, i) => (
            <div key={i} className="flex items-center gap-2 p-2 bg-muted/40 rounded-md">
              {action.type === 'Execute' && <Zap className="h-4 w-4 text-yellow-500" />}
              {action.type === 'Notify' && <Bell className="h-4 w-4 text-blue-500" />}
              {action.type === 'Log' && <FileText className="h-4 w-4 text-gray-500" />}
              <span className="text-xs px-2 py-1 bg-background rounded">
                {action.type === 'Execute' && '执行'}
                {action.type === 'Notify' && '通知'}
                {action.type === 'Log' && '日志'}
              </span>

              {action.type === 'Execute' && (
                <>
                  <Select value={action.device_id} onValueChange={v => {
                    const commands = getDeviceCommands(v, resources.devices, resources.deviceTypes)
                    updateAction(i, {
                      device_id: v,
                      command: commands[0]?.name || 'turn_on'
                    })
                  }}>
                    <SelectTrigger className="w-24 h-8">
                      <SelectValue />
                    </SelectTrigger>
                    <SelectContent>
                      {deviceOptions.map(d => (
                        <SelectItem key={d.value} value={d.value}>{d.label}</SelectItem>
                      ))}
                    </SelectContent>
                  </Select>
                  <Select value={action.command} onValueChange={v => updateAction(i, { command: v })}>
                    <SelectTrigger className="w-20 h-8">
                      <SelectValue />
                    </SelectTrigger>
                    <SelectContent>
                      {getDeviceCommands(action.device_id, resources.devices, resources.deviceTypes).map(c => (
                        <SelectItem key={c.name} value={c.name}>{c.display_name || c.name}</SelectItem>
                      ))}
                    </SelectContent>
                  </Select>
                </>
              )}

              {action.type === 'Notify' && (
                <Input
                  value={action.message}
                  onChange={e => updateAction(i, { message: e.target.value })}
                  placeholder="通知内容"
                  className="flex-1 h-8"
                />
              )}

              {action.type === 'Log' && (
                <>
                  <Select value={action.level} onValueChange={v => updateAction(i, { level: v as any })}>
                    <SelectTrigger className="w-14 h-8">
                      <SelectValue />
                    </SelectTrigger>
                    <SelectContent>
                      <SelectItem value="debug">DEBUG</SelectItem>
                      <SelectItem value="info">INFO</SelectItem>
                      <SelectItem value="warn">WARN</SelectItem>
                      <SelectItem value="error">ERROR</SelectItem>
                    </SelectContent>
                  </Select>
                  <Input
                    value={action.message}
                    onChange={e => updateAction(i, { message: e.target.value })}
                    placeholder="日志内容"
                    className="flex-1 h-8"
                  />
                </>
              )}

              <Button
                variant="ghost"
                size="icon"
                className="h-8 w-8 ml-auto"
                onClick={() => removeAction(i)}
              >
                ×
              </Button>
            </div>
          ))}
        </div>
      </FormSection>
    </div>
  )

  return (
    <ThreePaneBuilder
      open={open}
      onClose={() => onOpenChange(false)}
      title={rule ? '编辑自动化规则' : '创建自动化规则'}
      description="定义触发条件和执行动作，当条件满足时自动执行"
      icon={<Sparkles className="h-5 w-5 text-purple-500" />}
      chatPanel={{
        placeholder: '例如：当温度超过30度时打开空调...',
        onSendMessage: handleAiChat,
        generating: aiGenerating,
        clearOnClose: true,
      }}
      centerPanel={{
        title: '规则预览',
        content: (
          <RuleVisualization
            triggerType={triggerType}
            conditions={conditions}
            actions={actions}
            triggerState={triggerState}
            triggerCron={triggerCron}
            scheduleTime={scheduleTime}
            scheduleDays={scheduleDays}
            scheduleType={scheduleType}
            periodicType={periodicType}
            scheduleDayOfMonth={scheduleDayOfMonth}
            onceDateTime={onceDateTime}
            intervalValue={intervalValue}
            intervalUnit={intervalUnit}
            hasAiInput={aiGenerating || aiProcessed}
          />
        ),
      }}
      rightPanel={{
        title: '配置',
        content: rightPanelContent,
      }}
      isValid={isValid}
      isSaving={false}
      saveLabel="保存"
      onSave={handleSave}
      validationMessage={getValidationMessage()}
    />
  )
}
