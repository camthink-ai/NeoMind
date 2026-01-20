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
import { Switch } from '@/components/ui/switch'
import { Badge } from '@/components/ui/badge'
import { Tabs, TabsList, TabsTrigger, TabsContent } from '@/components/ui/tabs'
import {
  Plus,
  X,
  Wand2,
  Eye,
  Code,
  Calendar,
  Zap,
  Globe,
  Play,
  Bell,
  FileText,
  Trash2,
  Sparkles,
  Lightbulb,
  Info,
} from 'lucide-react'
import type { Rule, RuleTrigger, RuleCondition, RuleAction, DeviceType } from '@/types'
import {
  FullScreenBuilder,
  BuilderSection,
  FormGrid,
  TipCard,
} from './FullScreenBuilder'

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

type Mode = 'visual' | 'code' | 'ai'
type TriggerType = 'device_state' | 'schedule' | 'manual' | 'event'

const OPERATORS = [
  { value: '>', label: '大于', symbol: '>' },
  { value: '<', label: '小于', symbol: '<' },
  { value: '>=', label: '大于等于', symbol: '≥' },
  { value: '<=', label: '小于等于', symbol: '≤' },
  { value: '==', label: '等于', symbol: '=' },
  { value: '!=', label: '不等于', symbol: '≠' },
]

const AI_EXAMPLES = [
  '当温度超过30度时打开空调',
  '每天早上8点开灯',
  '当湿度低于40%时发送通知',
  '当设备离线时记录日志',
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
// Main Component
// ============================================================================

export function SimpleRuleBuilder({
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
  const [enabled, setEnabled] = useState(true)

  // Trigger state
  const [triggerType, setTriggerType] = useState<TriggerType>('manual')
  const [triggerDeviceId, setTriggerDeviceId] = useState('')
  const [triggerState, setTriggerState] = useState('')
  const [triggerCron, setTriggerCron] = useState('0 * * * *')
  const [triggerEventType, setTriggerEventType] = useState('')
  const [triggerEventFilters, setTriggerEventFilters] = useState('')

  // Conditions state
  const [conditions, setConditions] = useState<RuleCondition[]>([])

  // Actions state
  const [actions, setActions] = useState<RuleAction[]>([])

  // Mode and AI state
  const [mode, setMode] = useState<Mode>('visual')
  const [aiText, setAiText] = useState('')
  const [saving, setSaving] = useState(false)

  // ============================================================================
  // Initialize form from rule
  // ============================================================================
  useEffect(() => {
    if (open && rule) {
      setName(rule.name || '')
      setDescription(rule.description || '')
      setEnabled(rule.enabled ?? true)

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
    setEnabled(true)
    setTriggerType('manual')
    setTriggerDeviceId('')
    setTriggerState('')
    setTriggerCron('0 * * * *')
    setTriggerEventType('')
    setTriggerEventFilters('')
    setConditions([])
    setActions([{ type: 'Log', level: 'info', message: t('automation:logMessage', { defaultValue: '规则已触发' }) }])
    setAiText('')
    setMode('visual')
  }, [t])

  // ============================================================================
  // Trigger helpers
  // ============================================================================

  const buildTrigger = useCallback((): RuleTrigger | undefined => {
    switch (triggerType) {
      case 'device_state':
        if (!triggerDeviceId || !triggerState) return undefined
        return { type: 'device_state', device_id: triggerDeviceId, state: triggerState }
      case 'schedule':
        if (!triggerCron) return undefined
        return { type: 'schedule', cron: triggerCron }
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
  }, [triggerType, triggerDeviceId, triggerState, triggerCron, triggerEventType, triggerEventFilters])

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
      // Type-safe merge for discriminated union
      if (a.type === 'Notify' && data.type !== 'Notify') return { ...a, ...data } as RuleAction
      if (a.type === 'Execute' && data.type !== 'Execute') return { ...a, ...data } as RuleAction
      if (a.type === 'Log' && data.type !== 'Log') return { ...a, ...data } as RuleAction
      return { ...a, ...data } as RuleAction
    }))
  }, [])

  const removeAction = useCallback((index: number) => {
    setActions(prev => prev.filter((_, i) => i !== index))
  }, [])

  // ============================================================================
  // AI Generation
  // ============================================================================

  const handleAIGenerate = useCallback(async () => {
    if (!aiText.trim()) return

    // Simulate AI processing with regex-based parsing
    await new Promise(r => setTimeout(r, 600))

    const text = aiText.toLowerCase()

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
    const tempMatch = aiText.match(/温度.*?(\d+)/)
    const humMatch = aiText.match(/湿度.*?(\d+)/)

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

    if (!name.trim()) {
      setName(aiText.slice(0, 30))
    }

    setMode('visual')
  }, [aiText, name, resources.devices])

  // ============================================================================
  // Validation
  // ============================================================================

  const isValid = Boolean(name.trim() &&
    ((triggerType === 'manual') ||
     (triggerType === 'device_state' && triggerDeviceId) ||
     (triggerType === 'schedule' && triggerCron) ||
     (triggerType === 'event' && triggerEventType)))

  const getValidationMessage = () => {
    if (!name.trim()) return t('automation:validation.nameRequired', { defaultValue: '请输入规则名称' })
    if (triggerType === 'device_state' && !triggerDeviceId) return t('automation:validation.deviceRequired', { defaultValue: '请选择设备' })
    if (triggerType === 'schedule' && !triggerCron) return t('automation:validation.cronRequired', { defaultValue: '请输入 Cron 表达式' })
    if (triggerType === 'event' && !triggerEventType) return t('automation:validation.eventTypeRequired', { defaultValue: '请选择事件类型' })
    return ''
  }

  // ============================================================================
  // Save
  // ============================================================================

  const handleSave = async () => {
    if (!isValid) return

    setSaving(true)
    try {
      const trigger = buildTrigger()
      const condition = triggerType === 'device_state' && conditions.length > 0
        ? conditions[0]
        : undefined

      await onSave({
        name,
        description,
        enabled,
        trigger,
        condition,
        actions: actions.length > 0 ? actions : undefined,
      })
    } finally {
      setSaving(false)
    }
  }

  // Device options
  const deviceOptions = resources.devices.map(d => ({ value: d.id, label: d.name }))

  // ============================================================================
  // Side Panel Content
  // ============================================================================
  const sidePanelContent = (
    <div className="space-y-4">
      <TipCard
        title={t('automation:tips.ruleTitle', { defaultValue: '关于规则' })}
        variant="info"
      >
        {t('automation:tips.ruleDesc', {
          defaultValue: '规则是简单的"当...时..."自动化。当触发条件满足时，自动执行配置的动作。',
        })}
      </TipCard>

      {triggerType === 'device_state' && (
        <TipCard
          title={t('automation:tips.deviceStateTitle', { defaultValue: '设备状态触发' })}
          variant="info"
        >
          {t('automation:tips.deviceStateDesc', {
            defaultValue: '当指定设备的状态发生变化时触发规则。可以添加多个条件进行精确控制。',
          })}
        </TipCard>
      )}

      {triggerType === 'schedule' && (
        <TipCard
          title={t('automation:tips.scheduleTitle', { defaultValue: '定时触发' })}
          variant="info"
        >
          {t('automation:tips.scheduleDesc', {
            defaultValue: '使用 Cron 表达式设置定时触发。例如: 0 8 * * * 表示每天早上8点执行。',
          })}
        </TipCard>
      )}

      {triggerType === 'event' && (
        <TipCard
          title={t('automation:tips.eventTitle', { defaultValue: '事件触发' })}
          variant="info"
        >
          {t('automation:tips.eventDesc', {
            defaultValue: '响应系统事件，如设备上线/下线、其他规则触发等。',
          })}
        </TipCard>
      )}

      {mode === 'ai' && (
        <TipCard
          title={t('automation:tips.aiTitle', { defaultValue: 'AI 生成提示' })}
          variant="success"
        >
          {t('automation:tips.aiDesc', {
            defaultValue: '用自然语言描述你想要的自动化，AI 会自动配置触发器、条件和动作。',
          })}
        </TipCard>
      )}
    </div>
  )

  // ============================================================================
  // Render
  // ============================================================================
  return (
    <FullScreenBuilder
      open={open}
      onClose={() => onOpenChange(false)}
      title={rule
        ? t('automation:editRule', { defaultValue: '编辑规则' })
        : t('automation:createRule', { defaultValue: '创建自动化规则' })
      }
      description={t('automation:ruleBuilderDesc', {
        defaultValue: '定义触发条件和执行动作，当条件满足时自动执行',
      })}
      icon={<Sparkles className="h-5 w-5 text-purple-500" />}
      headerActions={
        <Badge variant={enabled ? 'default' : 'secondary'} className="text-xs">
          {enabled ? t('common:enabled', { defaultValue: '启用' }) : t('common:disabled', { defaultValue: '禁用' })}
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
              <Label htmlFor="rule-name">{t('automation:ruleName', { defaultValue: '规则名称' })} *</Label>
              <Input
                id="rule-name"
                value={name}
                onChange={e => setName(e.target.value)}
                placeholder={t('automation:ruleNamePlaceholder', { defaultValue: '例如：温度过高自动开空调' })}
              />
            </div>
            <div className="flex items-center gap-2 h-[42px]">
              <Switch checked={enabled} onCheckedChange={setEnabled} id="rule-enabled" />
              <Label htmlFor="rule-enabled" className="text-sm cursor-pointer">
                {t('automation:enableRule', { defaultValue: '启用规则' })}
              </Label>
            </div>
          </FormGrid>
          <div className="space-y-2">
            <Label htmlFor="rule-description">{t('common:description', { defaultValue: '描述' })}</Label>
            <Input
              id="rule-description"
              value={description}
              onChange={e => setDescription(e.target.value)}
              placeholder={t('automation:ruleDescPlaceholder', { defaultValue: '规则描述（可选）' })}
            />
          </div>
        </BuilderSection>

        {/* Mode Tabs */}
        <Tabs value={mode} onValueChange={(v) => setMode(v as Mode)} className="w-full">
          <TabsList className="grid w-full grid-cols-3">
            <TabsTrigger value="visual" className="gap-2">
              <Eye className="h-4 w-4" />
              <span>{t('automation:visualMode', { defaultValue: '可视化' })}</span>
            </TabsTrigger>
            <TabsTrigger value="code" className="gap-2">
              <Code className="h-4 w-4" />
              <span>{t('automation:codeMode', { defaultValue: '代码' })}</span>
            </TabsTrigger>
            <TabsTrigger value="ai" className="gap-2">
              <Wand2 className="h-4 w-4" />
              <span>{t('automation:aiMode', { defaultValue: 'AI 生成' })}</span>
            </TabsTrigger>
          </TabsList>

          {/* Visual Mode */}
          <TabsContent value="visual" className="mt-6 space-y-6">
            {/* Trigger Section */}
            <BuilderSection
              title={t('automation:trigger', { defaultValue: '触发器' })}
              description={t('automation:triggerDesc', { defaultValue: '选择何时触发此规则' })}
              icon={<Zap className="h-4 w-4 text-yellow-500" />}
            >
              {/* Trigger Type Buttons */}
              <div className="grid grid-cols-4 gap-3 mb-4">
                <Button
                  type="button"
                  variant={triggerType === 'manual' ? 'default' : 'outline'}
                  onClick={() => setTriggerType('manual')}
                  className="flex flex-col items-center gap-2 h-auto py-3"
                >
                  <Play className="h-5 w-5" />
                  <span className="text-sm">{t('automation:triggers.manual', { defaultValue: '手动' })}</span>
                </Button>
                <Button
                  type="button"
                  variant={triggerType === 'device_state' ? 'default' : 'outline'}
                  onClick={() => setTriggerType('device_state')}
                  className="flex flex-col items-center gap-2 h-auto py-3"
                >
                  <Zap className="h-5 w-5" />
                  <span className="text-sm">{t('automation:triggers.deviceState', { defaultValue: '设备状态' })}</span>
                </Button>
                <Button
                  type="button"
                  variant={triggerType === 'schedule' ? 'default' : 'outline'}
                  onClick={() => setTriggerType('schedule')}
                  className="flex flex-col items-center gap-2 h-auto py-3"
                >
                  <Calendar className="h-5 w-5" />
                  <span className="text-sm">{t('automation:triggers.schedule', { defaultValue: '定时' })}</span>
                </Button>
                <Button
                  type="button"
                  variant={triggerType === 'event' ? 'default' : 'outline'}
                  onClick={() => setTriggerType('event')}
                  className="flex flex-col items-center gap-2 h-auto py-3"
                >
                  <Globe className="h-5 w-5" />
                  <span className="text-sm">{t('automation:triggers.event', { defaultValue: '事件' })}</span>
                </Button>
              </div>

              {/* Device State Trigger Config */}
              {triggerType === 'device_state' && (
                <div className="space-y-3">
                  <Label className="text-sm text-muted-foreground">
                    {t('automation:deviceStateConfig', { defaultValue: '设备状态触发配置' })}
                  </Label>
                  <div className="flex gap-2">
                    <Select value={triggerDeviceId} onValueChange={setTriggerDeviceId}>
                      <SelectTrigger className="flex-1">
                        <SelectValue placeholder={t('automation:selectDevice', { defaultValue: '选择设备' })} />
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
                      placeholder={t('automation:stateValue', { defaultValue: '状态值 (如: active)' })}
                      className="flex-1"
                    />
                  </div>
                </div>
              )}

              {/* Schedule Trigger Config */}
              {triggerType === 'schedule' && (
                <div className="space-y-3">
                  <Label className="text-sm text-muted-foreground">
                    {t('automation:scheduleConfig', { defaultValue: '定时触发配置' })}
                  </Label>
                  <Input
                    value={triggerCron}
                    onChange={e => setTriggerCron(e.target.value)}
                    placeholder="* * * * *"
                    className="font-mono"
                  />
                  <p className="text-xs text-muted-foreground">
                    {t('automation:cronFormat', { defaultValue: '格式: 分 时 日 月 周 (例如: 0 * * * * = 每小时)' })}
                  </p>
                </div>
              )}

              {/* Event Trigger Config */}
              {triggerType === 'event' && (
                <div className="space-y-3">
                  <Label className="text-sm text-muted-foreground">
                    {t('automation:eventConfig', { defaultValue: '事件触发配置' })}
                  </Label>
                  <Select value={triggerEventType} onValueChange={setTriggerEventType}>
                    <SelectTrigger>
                      <SelectValue placeholder={t('automation:selectEventType', { defaultValue: '选择事件类型' })} />
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
            </BuilderSection>

            {/* Conditions Section */}
            {triggerType === 'device_state' && (
              <BuilderSection
                title={t('automation:conditions', { defaultValue: '触发条件' })}
                description={t('automation:conditionsDesc', { defaultValue: '满足条件时执行动作' })}
                icon={<Lightbulb className="h-4 w-4 text-yellow-500" />}
                actions={
                  <Button onClick={addCondition} variant="outline" size="sm">
                    <Plus className="h-4 w-4 mr-1" />
                    {t('automation:addCondition', { defaultValue: '添加条件' })}
                  </Button>
                }
              >
                {conditions.length === 0 ? (
                  <div className="text-center py-8 text-muted-foreground">
                    <Lightbulb className="h-8 w-8 mx-auto mb-2 opacity-50" />
                    <p className="text-sm">{t('automation:noConditions', { defaultValue: '暂无条件，点击上方按钮添加' })}</p>
                  </div>
                ) : (
                  <div className="space-y-2">
                    {conditions.map((cond, i) => {
                      const metrics = getDeviceMetrics(cond.device_id, resources.devices, resources.deviceTypes)
                      return (
                        <div key={i} className="flex items-center gap-2 p-3 bg-muted/40 rounded-md">
                          <span className="text-xs text-muted-foreground w-6">{i + 1}</span>
                          <Select value={cond.device_id} onValueChange={v => updateCondition(i, {
                            device_id: v,
                            metric: getDeviceMetrics(v, resources.devices, resources.deviceTypes)[0]?.name || 'temperature'
                          })}>
                            <SelectTrigger className="w-32">
                              <SelectValue />
                            </SelectTrigger>
                            <SelectContent>
                              {deviceOptions.map(d => (
                                <SelectItem key={d.value} value={d.value}>{d.label}</SelectItem>
                              ))}
                            </SelectContent>
                          </Select>
                          <span className="text-xs text-muted-foreground">的</span>
                          {metrics.length > 0 ? (
                            <Select value={cond.metric} onValueChange={v => updateCondition(i, { metric: v })}>
                              <SelectTrigger className="w-24">
                                <SelectValue />
                              </SelectTrigger>
                              <SelectContent>
                                {metrics.map(m => (
                                  <SelectItem key={m.name} value={m.name}>{m.display_name || m.name}</SelectItem>
                                ))}
                              </SelectContent>
                            </Select>
                          ) : (
                            <Input
                              value={cond.metric}
                              onChange={e => updateCondition(i, { metric: e.target.value })}
                              placeholder="指标"
                              className="w-24 h-9"
                            />
                          )}
                          <Select value={cond.operator} onValueChange={v => updateCondition(i, { operator: v as any })}>
                            <SelectTrigger className="w-16">
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
                            className="w-20 h-9"
                          />
                          <Button
                            variant="ghost"
                            size="icon"
                            className="h-8 w-8 ml-auto"
                            onClick={() => removeCondition(i)}
                          >
                            <X className="h-4 w-4" />
                          </Button>
                        </div>
                      )
                    })}
                  </div>
                )}
              </BuilderSection>
            )}

            {/* Actions Section */}
            <BuilderSection
              title={t('automation:actions', { defaultValue: '执行动作' })}
              description={t('automation:actionsDesc', { defaultValue: '触发时执行的操作' })}
              icon={<Play className="h-4 w-4 text-green-500" />}
            >
              {/* Add Action Buttons */}
              <div className="flex gap-2 mb-4">
                <Button onClick={() => addAction('Execute')} variant="outline" size="sm">
                  <Zap className="h-4 w-4 mr-1" />
                  {t('automation:executeCommand', { defaultValue: '执行命令' })}
                </Button>
                <Button onClick={() => addAction('Notify')} variant="outline" size="sm">
                  <Bell className="h-4 w-4 mr-1" />
                  {t('automation:sendNotification', { defaultValue: '发送通知' })}
                </Button>
                <Button onClick={() => addAction('Log')} variant="outline" size="sm">
                  <FileText className="h-4 w-4 mr-1" />
                  {t('automation:log', { defaultValue: '记录日志' })}
                </Button>
              </div>

              {actions.length === 0 ? (
                <div className="text-center py-8 text-muted-foreground">
                  <Play className="h-8 w-8 mx-auto mb-2 opacity-50" />
                  <p className="text-sm">{t('automation:noActions', { defaultValue: '暂无动作，点击上方按钮添加' })}</p>
                </div>
              ) : (
                <div className="space-y-2">
                  {actions.map((action, i) => (
                    <div key={i} className="flex items-center gap-2 p-3 bg-muted/40 rounded-md">
                      {action.type === 'Execute' && <Zap className="h-4 w-4 text-yellow-500" />}
                      {action.type === 'Notify' && <Bell className="h-4 w-4 text-blue-500" />}
                      {action.type === 'Log' && <FileText className="h-4 w-4 text-gray-500" />}
                      <span className="text-xs px-2 py-1 bg-background rounded">
                        {action.type === 'Execute' && t('automation:execute', { defaultValue: '执行' })}
                        {action.type === 'Notify' && t('automation:notify', { defaultValue: '通知' })}
                        {action.type === 'Log' && t('automation:log', { defaultValue: '日志' })}
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
                            <SelectTrigger className="w-28">
                              <SelectValue />
                            </SelectTrigger>
                            <SelectContent>
                              {deviceOptions.map(d => (
                                <SelectItem key={d.value} value={d.value}>{d.label}</SelectItem>
                              ))}
                            </SelectContent>
                          </Select>
                          <span className="text-xs text-muted-foreground">.</span>
                          <Select value={action.command} onValueChange={v => updateAction(i, { command: v })}>
                            <SelectTrigger className="w-24">
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
                          placeholder={t('automation:notificationMessage', { defaultValue: '通知内容' })}
                          className="flex-1"
                        />
                      )}

                      {action.type === 'Log' && (
                        <>
                          <Select value={action.level} onValueChange={v => updateAction(i, { level: v as any })}>
                            <SelectTrigger className="w-16">
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
                            placeholder={t('automation:logMessage', { defaultValue: '日志内容' })}
                            className="flex-1"
                          />
                        </>
                      )}

                      <Button
                        variant="ghost"
                        size="icon"
                        className="h-8 w-8 ml-auto"
                        onClick={() => removeAction(i)}
                      >
                        <Trash2 className="h-4 w-4" />
                      </Button>
                    </div>
                  ))}
                </div>
              )}
            </BuilderSection>
          </TabsContent>

          {/* Code Mode */}
          <TabsContent value="code" className="mt-6">
            <BuilderSection
              title={t('automation:ruleConfigJSON', { defaultValue: '规则配置 (JSON)' })}
              icon={<Code className="h-4 w-4 text-muted-foreground" />}
            >
              <Textarea
                readOnly
                value={JSON.stringify({
                  name,
                  description,
                  enabled,
                  trigger: buildTrigger(),
                  conditions,
                  actions,
                }, null, 2)}
                rows={16}
                className="font-mono text-sm"
              />
            </BuilderSection>
          </TabsContent>

          {/* AI Mode */}
          <TabsContent value="ai" className="mt-6 space-y-4">
            <BuilderSection
              title={t('automation:aiGenerate', { defaultValue: 'AI 智能生成' })}
              description={t('automation:aiGenerateDesc', { defaultValue: '用自然语言描述你想要的自动化规则' })}
              icon={<Wand2 className="h-4 w-4 text-purple-500" />}
            >
              <div className="space-y-4">
                <Textarea
                  value={aiText}
                  onChange={e => setAiText(e.target.value)}
                  placeholder={t('automation:aiRulePlaceholder', {
                    defaultValue: '例如：当温度超过30度时打开空调并发送通知',
                  })}
                  rows={4}
                  className="resize-none"
                />

                <div className="space-y-2">
                  <Label className="text-sm text-muted-foreground">
                    {t('automation:quickSelect', { defaultValue: '快速选择' })}
                  </Label>
                  <div className="flex flex-wrap gap-2">
                    {AI_EXAMPLES.map(ex => (
                      <Button
                        key={ex}
                        variant="outline"
                        size="sm"
                        type="button"
                        onClick={() => setAiText(ex)}
                        className="h-8 text-xs"
                      >
                        {ex}
                      </Button>
                    ))}
                  </div>
                </div>

                <Button className="w-full" onClick={handleAIGenerate} disabled={!aiText.trim()}>
                  <Wand2 className="h-4 w-4 mr-2" />
                  {t('automation:generateRule', { defaultValue: '生成规则' })}
                </Button>
              </div>
            </BuilderSection>
          </TabsContent>
        </Tabs>
      </div>
    </FullScreenBuilder>
  )
}
